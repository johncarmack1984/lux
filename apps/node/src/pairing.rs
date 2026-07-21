//! The unpaired-boot state machine (docs/claim-code-pairing.md, "Box:
//! first-boot state machine"). A factory-fresh headless box has no session and
//! no TTY to type a password into, so instead of dying it registers over plain
//! HTTPS and waits to be claimed from the lux app.
//!
//! Three legs, RFC 8628-shaped: register (`/auth/device/authorize`), poll
//! (`/auth/device/token`), redeem (the poll that comes back `granted`). Boot is
//! patient and never fatal — an unclaimed code simply expires and the box
//! re-registers with fresh codes, forever, so a box can sit unpaired on a shelf
//! and pair the moment someone opens Add-device on the same venue network.
//!
//! The reqwest + webpki idiom mirrors `setups.rs`: a headless box may not even
//! have `ca-certificates` installed, so we trust the bundled roots.

use std::time::{Duration, Instant};

use lux_wire::apple::AUTH_SEGMENT;
use lux_wire::device::{
    AuthorizeRequest, AuthorizeResponse, TokenRequest, TokenResponse, AUTHORIZE_SEGMENT,
    DEVICE_SEGMENT, TOKEN_SEGMENT,
};

use lux_engine::tls::webpki_pem_bundle;

use crate::config::{Endpoints, StoredSession};

/// Brief settle before re-registering after an expired/denied code, plus jitter
/// so a fleet of boxes booted together don't re-register in lockstep.
const REREGISTER_BASE: Duration = Duration::from_secs(2);
const REREGISTER_JITTER_SECS: u64 = 3;
/// Cap on the transient-error (network/5xx) backoff during registration.
const NET_BACKOFF_CAP_SECS: u64 = 30;
/// Ceiling on the poll interval after `slow_down` bumps.
const MAX_INTERVAL_SECS: u64 = 60;

/// A completed grant: everything the caller persists (`session.json` +
/// `node.json`) before falling through to the normal connect path.
pub struct Granted {
    pub session: StoredSession,
    pub setup_id: String,
    pub universe: u16,
}

/// What a `/token` poll status means for the loop. Pure so the transitions are
/// unit-tested without a network.
#[derive(Debug, PartialEq, Eq)]
enum PollAction {
    /// `authorization_pending` — keep polling at the current interval.
    KeepPolling,
    /// `slow_down` (RFC 8628 §3.5) — widen the interval, keep polling.
    SlowDown,
    /// `expired_token` / `access_denied` / anything unexpected — drop the codes
    /// and register afresh. The patient boot never gives up.
    ReRegister,
    /// `granted` — the poll carries the session fields.
    Granted,
}

fn classify(status: &str) -> PollAction {
    match status {
        "authorization_pending" => PollAction::KeepPolling,
        "slow_down" => PollAction::SlowDown,
        "granted" => PollAction::Granted,
        _ => PollAction::ReRegister,
    }
}

/// Announce a fresh code to the journal — SSH-fallback parity with the app's
/// display. Used by `run`'s unpaired path.
pub fn journal_announce(auth: &AuthorizeResponse) {
    log::info!(
        "lux-node is unpaired: open the lux app (Settings > Devices > Add device) \
         and confirm code {} to claim this box",
        auth.user_code
    );
}

/// Announce a fresh code to stdout — the `lux-node pair` CLI (a human with a
/// shell). Journald also captures this, so parity is preserved either way.
pub fn stdout_announce(auth: &AuthorizeResponse) {
    println!();
    println!("  lux-node is waiting to be paired.");
    println!("  In the lux app: Settings > Devices > Add device, then confirm this code:");
    println!();
    println!("      {}", auth.user_code);
    println!();
    println!("  (waiting… the code refreshes automatically until you approve it)");
    println!();
}

/// Register, then poll until claimed. Blocks (patiently) forever on the wait;
/// only a completed grant or an unrecoverable local error returns.
pub async fn pair_wait(
    env: &Endpoints,
    announce: impl Fn(&AuthorizeResponse),
) -> Result<Granted, String> {
    let device_id = crate::config::load_or_create_device_id()?;
    let request = device_request(device_id);
    let client = http_client(env)?;
    let mut net_backoff = 1u64;

    'register: loop {
        let auth = match authorize(&client, env, &request).await {
            Ok(auth) => {
                net_backoff = 1;
                auth
            }
            Err(e) => {
                log::warn!("device register failed ({e}); retrying in {net_backoff}s");
                sleep_secs(net_backoff, REREGISTER_JITTER_SECS).await;
                net_backoff = (net_backoff * 2).min(NET_BACKOFF_CAP_SECS);
                continue 'register;
            }
        };
        announce(&auth);

        let deadline = Instant::now() + Duration::from_secs(u64::from(auth.expires_in));
        let mut interval = u64::from(auth.interval.max(1)).min(MAX_INTERVAL_SECS);

        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            if Instant::now() >= deadline {
                log::info!("pairing code expired unclaimed; re-registering");
                break;
            }
            match poll_once(&client, env, &auth.device_code).await {
                Ok(resp) => match classify(&resp.status) {
                    PollAction::KeepPolling => {}
                    PollAction::SlowDown => interval = (interval + 5).min(MAX_INTERVAL_SECS),
                    PollAction::ReRegister => {
                        log::info!(
                            "pairing grant no longer valid ({}); re-registering",
                            resp.status
                        );
                        break;
                    }
                    PollAction::Granted => {
                        return granted_from(resp, &env.cognito_device_client_id);
                    }
                },
                // Transient (unreachable service / 5xx): keep polling. Never
                // fatal — the box waits out the outage and pairs when it clears.
                Err(e) => log::warn!("pairing poll failed ({e}); retrying"),
            }
        }

        sleep_secs(REREGISTER_BASE.as_secs(), REREGISTER_JITTER_SECS).await;
    }
}

/// Build the `AuthorizeRequest` display metadata once — it never changes for a
/// given boot, so a re-registering node reuses it (same `device_id` supersedes
/// its own earlier codes).
fn device_request(device_id: String) -> AuthorizeRequest {
    AuthorizeRequest {
        device_id,
        hostname: gethostname::gethostname().to_string_lossy().into_owned(),
        mac_tail: mac_tail(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
}

/// Turn a `granted` poll into the persisted grant. Fails closed if the response
/// is missing a field the node cannot invent.
fn granted_from(resp: TokenResponse, device_client_id: &str) -> Result<Granted, String> {
    let email = resp.email.ok_or("granted response missing email")?;
    let refresh_token = resp
        .refresh_token
        .ok_or("granted response missing refreshToken")?;
    let setup_id = resp.setup_id.ok_or("granted response missing setupId")?;
    let universe = resp.universe.unwrap_or(1);
    // The backend always names the minting (device) client; default to the
    // configured one so a session is never written that would refresh against
    // the wrong client and get rejected.
    let client_id = resp.client_id.or_else(|| Some(device_client_id.to_owned()));
    Ok(Granted {
        session: StoredSession {
            email,
            refresh_token,
            client_id,
        },
        setup_id,
        universe,
    })
}

async fn authorize(
    client: &reqwest::Client,
    env: &Endpoints,
    request: &AuthorizeRequest,
) -> Result<AuthorizeResponse, String> {
    let resp = client
        .post(device_url(env, AUTHORIZE_SEGMENT))
        .json(request)
        .send()
        .await
        .map_err(|e| format!("auth service unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("authorize answered {}", resp.status()));
    }
    resp.json()
        .await
        .map_err(|e| format!("bad authorize reply: {e}"))
}

/// One `/token` poll. The backend returns a `TokenResponse` body for the
/// negative statuses too (`expired_token`/`access_denied` ride HTTP 400), so we
/// parse the body regardless of status code and let [`classify`] decide.
async fn poll_once(
    client: &reqwest::Client,
    env: &Endpoints,
    device_code: &str,
) -> Result<TokenResponse, String> {
    let resp = client
        .post(device_url(env, TOKEN_SEGMENT))
        .json(&TokenRequest {
            device_code: device_code.to_owned(),
        })
        .send()
        .await
        .map_err(|e| format!("auth service unreachable: {e}"))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("token poll {status}: {e}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("token poll {status}: unreadable body: {e}"))
}

fn http_client(env: &Endpoints) -> Result<reqwest::Client, String> {
    let certs = reqwest::Certificate::from_pem_bundle(webpki_pem_bundle())
        .map_err(|e| format!("webpki bundle: {e}"))?;
    let mut builder = reqwest::Client::builder().tls_certs_only(certs);
    // Prefer IPv4 for the rendezvous: the shared NAT address is the identity the
    // approving app can reliably match on a dual-stack network (lux_engine::net).
    // Best-effort — a resolution miss leaves reqwest's default dual-stack DNS.
    if let Some((host, port)) = host_port(&env.apple_auth_url) {
        let addrs = lux_engine::net::ipv4_first_addrs(&host, port);
        if !addrs.is_empty() {
            builder = builder.resolve_to_addrs(&host, &addrs);
        }
    }
    builder.build().map_err(|e| e.to_string())
}

/// Host + port (defaulting 443) from a base URL, for a reqwest resolver override.
fn host_port(base_url: &str) -> Option<(String, u16)> {
    let url = reqwest::Url::parse(base_url).ok()?;
    let host = url.host_str()?.to_owned();
    Some((host, url.port_or_known_default().unwrap_or(443)))
}

/// `<appleAuthUrl>/auth/device/<segment>`.
fn device_url(env: &Endpoints, segment: &str) -> String {
    let base = env.apple_auth_url.trim_end_matches('/');
    format!("{base}/{AUTH_SEGMENT}/{DEVICE_SEGMENT}/{segment}")
}

/// Sleep `base` seconds plus up to `jitter_secs` of jitter.
async fn sleep_secs(base: u64, jitter_secs: u64) {
    tokio::time::sleep(Duration::from_secs(base) + jitter(jitter_secs)).await;
}

/// A cheap, dependency-free jitter: the wall-clock sub-second, so a fleet that
/// booted together doesn't re-register in lockstep. Precision is irrelevant.
fn jitter(max_secs: u64) -> Duration {
    if max_secs == 0 {
        return Duration::ZERO;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    Duration::from_millis(u64::from(nanos) % (max_secs * 1000))
}

/// Last four hex digits of the box's primary MAC — the approve screen's
/// physical cross-check against the sticker/port label. Best-effort: an
/// unreadable NIC set degrades to `0000` rather than blocking a boot.
fn mac_tail() -> String {
    let mac = read_primary_mac().unwrap_or_default();
    let hex: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase();
    if hex.len() >= 4 {
        hex[hex.len() - 4..].to_owned()
    } else {
        "0000".to_owned()
    }
}

/// Pick the box's primary MAC from sysfs: prefer wired (`en*`/`eth*`), then
/// wireless (`wl*`), then anything else, tie-broken by name for a stable
/// choice across boots. Loopback and all-zero addresses are skipped.
#[cfg(target_os = "linux")]
fn read_primary_mac() -> Option<String> {
    let mut best: Option<(u8, String, String)> = None;
    for entry in std::fs::read_dir("/sys/class/net").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(entry.path().join("address")) else {
            continue;
        };
        let mac = raw.trim().to_owned();
        if mac.is_empty() || mac.chars().all(|c| c == '0' || c == ':') {
            continue;
        }
        let prio = if name.starts_with("en") || name.starts_with("eth") {
            0
        } else if name.starts_with("wl") {
            1
        } else {
            2
        };
        let better = match &best {
            None => true,
            Some((bp, bn, _)) => prio < *bp || (prio == *bp && name < *bn),
        };
        if better {
            best = Some((prio, name, mac));
        }
    }
    best.map(|(_, _, mac)| mac)
}

#[cfg(not(target_os = "linux"))]
fn read_primary_mac() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_status_transitions() {
        assert_eq!(classify("authorization_pending"), PollAction::KeepPolling);
        assert_eq!(classify("slow_down"), PollAction::SlowDown);
        assert_eq!(classify("expired_token"), PollAction::ReRegister);
        assert_eq!(classify("access_denied"), PollAction::ReRegister);
        assert_eq!(classify("granted"), PollAction::Granted);
        // Anything the box doesn't recognise is treated as re-register, never
        // as a fatal boot error.
        assert_eq!(classify("something_new"), PollAction::ReRegister);
    }

    fn granted_response() -> TokenResponse {
        TokenResponse {
            status: "granted".into(),
            email: Some("venue@example.com".into()),
            refresh_token: Some("refresh-xyz".into()),
            client_id: Some("device-client".into()),
            setup_id: Some("setup-1".into()),
            universe: Some(3),
        }
    }

    #[test]
    fn granted_maps_to_session_and_binding() {
        let g = granted_from(granted_response(), "fallback-client").expect("granted maps");
        assert_eq!(g.session.email, "venue@example.com");
        assert_eq!(g.session.refresh_token, "refresh-xyz");
        assert_eq!(g.session.client_id.as_deref(), Some("device-client"));
        assert_eq!(g.setup_id, "setup-1");
        assert_eq!(g.universe, 3);
    }

    #[test]
    fn granted_defaults_universe_and_client() {
        let resp = TokenResponse {
            client_id: None,
            universe: None,
            ..granted_response()
        };
        let g = granted_from(resp, "fallback-client").expect("granted maps");
        // Universe defaults to 1; the client falls back to the configured
        // device client so refresh never targets the interactive client.
        assert_eq!(g.universe, 1);
        assert_eq!(g.session.client_id.as_deref(), Some("fallback-client"));
    }

    #[test]
    fn granted_requires_the_session_fields() {
        for missing in ["email", "refreshToken", "setupId"] {
            let mut resp = granted_response();
            match missing {
                "email" => resp.email = None,
                "refreshToken" => resp.refresh_token = None,
                "setupId" => resp.setup_id = None,
                _ => unreachable!(),
            }
            assert!(
                granted_from(resp, "fallback-client").is_err(),
                "a granted response without {missing} must fail closed"
            );
        }
    }

    #[test]
    fn mac_tail_is_four_lowercase_hex() {
        let tail = mac_tail();
        assert_eq!(tail.len(), 4);
        assert!(tail.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(tail, tail.to_lowercase());
    }

    #[test]
    fn device_url_joins_the_segments() {
        let env = crate::config::endpoints().expect("endpoints");
        let url = device_url(&env, AUTHORIZE_SEGMENT);
        assert!(url.ends_with("/auth/device/authorize"), "got {url}");
        assert!(!url.contains("//auth"), "no double slash: {url}");
    }
}
