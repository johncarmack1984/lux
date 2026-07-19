//! Headless device pairing — the RFC 8628-shaped grant from
//! docs/claim-code-pairing.md, served next to the Apple routes because the
//! trust machinery is the same: the pool's CUSTOM_AUTH triggers (this binary)
//! are the verifier, these routes only add reach.
//!
//! The rendezvous is same-public-egress: an unpaired box registers over plain
//! HTTPS and the approve screen only ever lists registrations that arrived
//! from the caller's own public IP — phone on the venue WiFi sees the box on
//! the venue ethernet, and nobody else sees either. Identity is never claimed
//! by the device; it is granted by the approving account, and the mint runs
//! on the device app client so appliance sessions stay segregated from
//! interactive ones.
//!
//! Every route (and the trigger's device path) fails closed unless
//! `COGNITO_DEVICE_CLIENT_ID` is configured.

use std::sync::Arc;

use lambda_runtime::Error;
use lux_wire::device::{
    ApproveRequest, ApproveResponse, AuthorizeRequest, AuthorizeResponse, DeviceRecord,
    ListResponse, PendingDevice, PendingResponse, RevokeRequest, RevokeResponse, TokenRequest,
    TokenResponse,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::http::{caller, error, parse_body, reply, UrlEvent};
use crate::{cognito, store, Ctx};

/// RFC 8628 §3.2: how often the node may poll `/token`.
const INTERVAL_SECS: u32 = 5;
/// Code-pair lifetime; an unclaimed node re-registers after this.
const EXPIRES_SECS: u32 = 900;
/// How long a redeemed grant stays mintable — covers exactly the token call
/// in flight, nothing more.
const REDEEM_WINDOW_MILLIS: i64 = 60_000;

/// Display-code alphabet: no vowels (no words), no 0/O/1/I/L/S lookalikes.
const USER_CODE_ALPHABET: &[u8] = b"23456789CDFGHJKMNPQRTVWXZ";
const USER_CODE_LEN: usize = 4;

/// `POST /auth/device/authorize` — an unpaired node registers and receives its
/// code pair. Unauthenticated by design; everything in the body is display
/// metadata, and the reply grants nothing.
pub async fn authorize(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    if ctx.device_client_id.is_none() {
        return reply(503, &error("device pairing is not enabled"));
    }
    let req: AuthorizeRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };
    let pub_ip = event.source_ip();
    if pub_ip.is_empty() {
        return reply(400, &error("no source address"));
    }

    let device_code = match random_hex::<32>() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{e}");
            return reply(500, &error("internal"));
        }
    };
    let user_code = match user_code() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("{e}");
            return reply(500, &error("internal"));
        }
    };

    if let Err(e) = store::create_pair(
        ctx,
        &pair_ref(&device_code),
        &user_code,
        &req.device_id,
        &req.hostname,
        &req.mac_tail,
        &req.version,
        &req.arch,
        pub_ip,
        EXPIRES_SECS as i64,
    )
    .await
    {
        tracing::error!("pair create failed: {e}");
        return reply(500, &error("internal"));
    }

    reply(
        200,
        &AuthorizeResponse {
            device_code,
            user_code,
            interval: INTERVAL_SECS,
            expires_in: EXPIRES_SECS,
        },
    )
}

/// `POST /auth/device/token` — the node's poll. Unknown, expired, denied, and
/// already-redeemed codes all collapse to `access_denied`: the caller learns
/// nothing a legitimate device wouldn't already know.
pub async fn token(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(device_client_id) = ctx.device_client_id.as_deref() else {
        return reply(503, &error("device pairing is not enabled"));
    };
    let req: TokenRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };

    let pair = match store::get_pair(ctx, &pair_ref(&req.device_code)).await {
        Err(e) => {
            tracing::error!("pair get failed: {e}");
            return reply(500, &error("internal"));
        }
        Ok(None) => return status(400, "access_denied"),
        Ok(Some(p)) => p,
    };

    match pair.status.as_str() {
        "pending" if pair.expired() => status(400, "expired_token"),
        "pending" => status(200, "authorization_pending"),
        "approved" => grant(ctx, device_client_id, &req.device_code, pair).await,
        _ => status(400, "access_denied"),
    }
}

/// The approved poll: claim the single-use flip, then mint on the device
/// client. The Verify trigger (this binary) independently re-checks the code
/// against the store, so a direct-to-Cognito caller gets nothing extra.
async fn grant(
    ctx: &Arc<Ctx>,
    device_client_id: &str,
    device_code: &str,
    pair: store::Pair,
) -> Result<Value, Error> {
    let (Some(username), Some(setup_id)) = (pair.bound_username.clone(), pair.setup_id.clone())
    else {
        tracing::error!("approved pair missing binding");
        return reply(500, &error("internal"));
    };

    // Exactly one poll wins this; a failure after it burns the code and the
    // node re-registers (safety over convenience).
    if let Err(e) = store::redeem_pair(ctx, &pair_ref(device_code)).await {
        tracing::warn!("redeem race lost or stale: {e}");
        return status(400, "access_denied");
    }

    let tokens = match cognito::custom_auth(ctx, device_client_id, &username, device_code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("device mint failed: {e}");
            return reply(500, &error("internal"));
        }
    };

    // The email attribute, not the username — Apple-created accounts have
    // UUID usernames and the node stores/logs this as the account identity.
    let email = match cognito::email_of(ctx, &username).await {
        Ok(Some(e)) => e,
        Ok(None) => username,
        Err(e) => {
            tracing::warn!("email lookup failed, falling back to username: {e}");
            username
        }
    };

    reply(
        200,
        &TokenResponse {
            status: "granted".into(),
            email: Some(email),
            refresh_token: Some(tokens.refresh_token),
            client_id: Some(device_client_id.to_owned()),
            setup_id: Some(setup_id),
            universe: Some(pair.universe.unwrap_or(1)),
        },
    )
}

/// `GET /auth/device/list` — bearer-authed: the caller's paired devices.
pub async fn list(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(caller_sub) = caller(ctx, event) else {
        return reply(401, &error("invalid or missing token"));
    };
    let rows = match store::list_devices(ctx, &caller_sub).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("device list failed: {e}");
            return reply(500, &error("internal"));
        }
    };
    let devices: Vec<DeviceRecord> = rows
        .iter()
        .filter(|item| item.get("revoked").and_then(|v| v.as_bool().ok()) != Some(&true))
        .filter_map(|item| {
            let s = |k: &str| item.get(k)?.as_s().ok().cloned();
            let n = |k: &str| {
                item.get(k)
                    .and_then(|v| v.as_n().ok())
                    .and_then(|v| v.parse::<i64>().ok())
            };
            Some(DeviceRecord {
                device_id: item.get("sk")?.as_s().ok()?.clone(),
                name: s("name")?,
                hostname: s("hostname")?,
                setup_id: s("setupId")?,
                universe: n("universe")? as u16,
                paired_at: n("pairedAt")?,
            })
        })
        .collect();
    reply(200, &ListResponse { devices })
}

/// `POST /auth/device/revoke` — bearer-authed: the owner removes one of their
/// paired devices. Data-plane only in v1: the device drops out of `/list` at
/// once (a revoked registry row), but cutting the box's live IoT access is
/// authorizer-level enforcement, deferred by design. Removing a device the
/// caller doesn't own is an idempotent no-op (`revoked: false`), never an error.
pub async fn revoke(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(caller_sub) = caller(ctx, event) else {
        return reply(401, &error("invalid or missing token"));
    };
    let req: RevokeRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };
    match store::revoke_device(ctx, &caller_sub, &req.device_id).await {
        Ok(revoked) => reply(200, &RevokeResponse { revoked }),
        Err(e) => {
            tracing::error!("device revoke failed: {e}");
            reply(500, &error("internal"))
        }
    }
}

/// `GET /auth/device/pending` — bearer-authed: unexpired registrations that
/// arrived from the caller's own public egress, oldest first.
pub async fn pending(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    if ctx.device_client_id.is_none() {
        return reply(503, &error("device pairing is not enabled"));
    }
    if caller(ctx, event).is_none() {
        return reply(401, &error("invalid or missing token"));
    }
    let rows = match store::list_pending(ctx, event.source_ip()).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("pending list failed: {e}");
            return reply(500, &error("internal"));
        }
    };
    let devices: Vec<PendingDevice> = rows
        .iter()
        .filter_map(|item| {
            let s = |k: &str| item.get(k)?.as_s().ok().cloned();
            Some(PendingDevice {
                pair_ref: s("pairRef")?,
                user_code: s("userCode")?,
                hostname: s("hostname")?,
                mac_tail: s("macTail")?,
                version: s("version")?,
                arch: s("arch")?,
                first_seen: item
                    .get("createdAt")
                    .and_then(|v| v.as_n().ok())
                    .and_then(|v| v.parse().ok())?,
            })
        })
        .collect();
    reply(200, &PendingResponse { devices })
}

/// `POST /auth/device/approve` — bearer-authed. The same-egress check runs
/// again here (not just on the listing): an approval must come from the same
/// public IP the box registered from.
pub async fn approve(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    if ctx.device_client_id.is_none() {
        return reply(503, &error("device pairing is not enabled"));
    }
    let Some(caller_sub) = caller(ctx, event) else {
        return reply(401, &error("invalid or missing token"));
    };
    let req: ApproveRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };

    let pair = match store::get_pair(ctx, &req.pair_ref).await {
        Err(e) => {
            tracing::error!("pair get failed: {e}");
            return reply(500, &error("internal"));
        }
        Ok(None) => return reply(404, &error("no such pending device")),
        Ok(Some(p)) => p,
    };
    if pair.status != "pending" || pair.expired() {
        return reply(409, &error("device is no longer pending"));
    }
    if pair.pub_ip != event.source_ip() {
        tracing::warn!("approve egress mismatch");
        return reply(403, &error("approve from the device's network"));
    }

    let user = match cognito::find_user_by_sub(ctx, &caller_sub).await {
        Ok(Some(u)) => u,
        Ok(None) => return reply(404, &error("account not found")),
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            return reply(500, &error("internal"));
        }
    };

    let universe = req.universe.unwrap_or(1);
    let name = req.name.as_deref().unwrap_or(&pair.hostname);
    if let Err(e) = store::approve_pair(
        ctx,
        &req.pair_ref,
        &pair,
        &user.username,
        &user.sub,
        &req.setup_id,
        universe,
        name,
    )
    .await
    {
        // A concurrent approve (or expiry) losing the condition is a 409, not
        // an internal error — but we can't tell them apart cheaply; log both.
        tracing::warn!("pair approve rejected: {e}");
        return reply(409, &error("device is no longer pending"));
    }

    reply(200, &ApproveResponse { approved: true })
}

/// Is this challenge answer a live, just-redeemed device code bound to exactly
/// this user? The trigger's device path — the actual trust anchor of the mint.
pub async fn answer_is_correct(ctx: &Ctx, answer: &str, user_sub: &str) -> bool {
    let pair = match store::get_pair(ctx, &pair_ref(answer)).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::warn!("device challenge answer matches no pair");
            return false;
        }
        Err(e) => {
            tracing::error!("pair lookup failed during verify: {e}");
            return false;
        }
    };
    let fresh = pair
        .redeemed_at
        .is_some_and(|at| now_millis().saturating_sub(at) < REDEEM_WINDOW_MILLIS);
    pair.status == "redeemed" && fresh && pair.bound_sub.as_deref() == Some(user_sub)
}

// --- codes ----------------------------------------------------------------------

/// The at-rest key for a device code: its hex sha256. The secret itself never
/// touches the table.
fn pair_ref(device_code: &str) -> String {
    hex(&Sha256::digest(device_code.as_bytes()))
}

/// `LUX-XXXX` from the confusion-free alphabet. Display-only (never typed, is
/// no authority), so the tiny modulo bias is irrelevant.
fn user_code() -> Result<String, String> {
    let bytes = random::<USER_CODE_LEN>()?;
    let code: String = bytes
        .iter()
        .map(|b| USER_CODE_ALPHABET[*b as usize % USER_CODE_ALPHABET.len()] as char)
        .collect();
    Ok(format!("LUX-{code}"))
}

fn random_hex<const N: usize>() -> Result<String, String> {
    Ok(hex(&random::<N>()?))
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    use std::io::Read;
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no OS randomness: {e}"))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn status(code: u16, s: &str) -> Result<Value, Error> {
    reply(
        code,
        &TokenResponse {
            status: s.into(),
            email: None,
            refresh_token: None,
            client_id: None,
            setup_id: None,
            universe: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_codes_use_the_display_alphabet() {
        let code = user_code().unwrap();
        assert!(code.starts_with("LUX-"));
        let tail = &code["LUX-".len()..];
        assert_eq!(tail.len(), USER_CODE_LEN);
        assert!(tail.bytes().all(|b| USER_CODE_ALPHABET.contains(&b)));
    }

    #[test]
    fn pair_ref_is_a_stable_hash_not_the_secret() {
        let secret = "aaaaaaaa";
        let r = pair_ref(secret);
        assert_eq!(r.len(), 64);
        assert_eq!(r, pair_ref(secret));
        assert!(!r.contains(secret));
    }
}
