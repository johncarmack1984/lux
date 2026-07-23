//! The Function URL surface: parse the Lambda URL (payload v2) event, route,
//! and reply in `lux-wire` shapes. Identity on `/link` and `/revoke` comes only
//! from the verified Cognito bearer token; identity on `/auth/apple` comes only
//! from the verified Apple identity token — a request body never names a user.

use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use lambda_runtime::Error;
use lux_wire::apple::{
    LinkResponse, RevokeResponse, SignInRequest, SignInResponse, APPLE_SEGMENT, AUTH_SEGMENT,
    CALLBACK_SEGMENT, EXCHANGE_SEGMENT, LINK_SEGMENT, REVOKE_SEGMENT, START_SEGMENT, WEB_SEGMENT,
};
use lux_wire::ErrorResponse;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{apple, cognito, store, Ctx};

/// The slice of a Function URL (payload format 2.0) event we route on.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct UrlEvent {
    raw_path: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    is_base64_encoded: bool,
    request_context: RequestContext,
}

impl UrlEvent {
    /// The caller's public IP as the Function URL saw it — the pairing flow's
    /// same-egress rendezvous key.
    pub(crate) fn source_ip(&self) -> &str {
        &self.request_context.http.source_ip
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RequestContext {
    http: HttpContext,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HttpContext {
    method: String,
    source_ip: String,
}

pub async fn handle(ctx: &Arc<Ctx>, payload: Value) -> Result<Value, Error> {
    let event: UrlEvent = match serde_json::from_value(payload) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("unroutable invoke payload: {e}");
            return reply(400, &error("unroutable request"));
        }
    };

    let method = event.request_context.http.method.as_str();
    let path = event.raw_path.clone();
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();

    use lux_wire::device::{
        APPROVE_SEGMENT, AUTHORIZE_SEGMENT, DEVICE_SEGMENT, LIST_SEGMENT, PENDING_SEGMENT,
        REVOKE_SEGMENT as DEVICE_REVOKE_SEGMENT, TOKEN_SEGMENT,
    };

    match (method, segments.as_slice()) {
        ("POST", [a, b]) if *a == AUTH_SEGMENT && *b == APPLE_SEGMENT => sign_in(ctx, &event).await,
        ("POST", [a, b, c]) if *a == AUTH_SEGMENT && *b == APPLE_SEGMENT && *c == LINK_SEGMENT => {
            link(ctx, &event).await
        }
        ("POST", [a, b, c])
            if *a == AUTH_SEGMENT && *b == APPLE_SEGMENT && *c == REVOKE_SEGMENT =>
        {
            revoke(ctx, &event).await
        }
        ("POST", [a, b, c, d])
            if *a == AUTH_SEGMENT
                && *b == APPLE_SEGMENT
                && *c == WEB_SEGMENT
                && *d == START_SEGMENT =>
        {
            crate::web::start(ctx, &event).await
        }
        ("POST", [a, b, c, d])
            if *a == AUTH_SEGMENT
                && *b == APPLE_SEGMENT
                && *c == WEB_SEGMENT
                && *d == CALLBACK_SEGMENT =>
        {
            crate::web::callback(ctx, &event).await
        }
        ("POST", [a, b, c, d])
            if *a == AUTH_SEGMENT
                && *b == APPLE_SEGMENT
                && *c == WEB_SEGMENT
                && *d == EXCHANGE_SEGMENT =>
        {
            crate::web::exchange(ctx, &event).await
        }
        ("POST", [a, b, c])
            if *a == AUTH_SEGMENT && *b == DEVICE_SEGMENT && *c == AUTHORIZE_SEGMENT =>
        {
            crate::device::authorize(ctx, &event).await
        }
        ("POST", [a, b, c])
            if *a == AUTH_SEGMENT && *b == DEVICE_SEGMENT && *c == TOKEN_SEGMENT =>
        {
            crate::device::token(ctx, &event).await
        }
        ("GET", [a, b, c])
            if *a == AUTH_SEGMENT && *b == DEVICE_SEGMENT && *c == PENDING_SEGMENT =>
        {
            crate::device::pending(ctx, &event).await
        }
        ("GET", [a, b, c]) if *a == AUTH_SEGMENT && *b == DEVICE_SEGMENT && *c == LIST_SEGMENT => {
            crate::device::list(ctx, &event).await
        }
        ("POST", [a, b, c])
            if *a == AUTH_SEGMENT && *b == DEVICE_SEGMENT && *c == APPROVE_SEGMENT =>
        {
            crate::device::approve(ctx, &event).await
        }
        ("POST", [a, b, c])
            if *a == AUTH_SEGMENT && *b == DEVICE_SEGMENT && *c == DEVICE_REVOKE_SEGMENT =>
        {
            crate::device::revoke(ctx, &event).await
        }
        _ => reply(404, &error("not found")),
    }
}

/// `POST /auth/apple` — sign in with a verified Apple identity token, creating
/// or auto-linking a Cognito user on first use.
async fn sign_in(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let req: SignInRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };

    let identity = match ctx
        .apple
        .verify_identity(&req.identity_token, &req.raw_nonce)
        .await
    {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("apple identity token rejected: {e}");
            return reply(401, &error("invalid apple identity token"));
        }
    };

    let existing = match store::get_link(ctx, &identity.sub).await {
        Err(e) => {
            tracing::error!("link lookup failed: {e}");
            return reply(500, &error("internal"));
        }
        Ok(link) => link,
    };
    let (mut username, mut created) = match &existing {
        Some(link) => {
            // Known Apple user. Refresh the stored (revocable) Apple token
            // best-effort — the sign-in itself must not depend on Apple's
            // token endpoint being reachable once a link exists.
            match refresh_apple_token(
                ctx,
                &identity.sub,
                &req.authorization_code,
                ctx.apple.bundle_id(),
            )
            .await
            {
                Ok(()) => {}
                Err(e) => tracing::warn!("apple refresh-token update skipped: {e}"),
            }
            (link.username.clone(), false)
        }
        None => match first_link_native(ctx, &identity, &req).await {
            Ok(x) => x,
            Err(e) => return Ok(e),
        },
    };

    let mut attempt =
        cognito::custom_auth(ctx, &ctx.client_id, &username, &req.identity_token).await;
    if let (Err(cognito::AuthError::UserNotFound), Some(link)) = (&attempt, &existing) {
        // The linked user is gone — an account deletion whose Apple-side
        // revoke never landed. Self-heal: drop the stale link and run this as
        // a first sign-in (the same credential simply creates or relinks).
        tracing::warn!("apple link points at a deleted user; relinking");
        if let Err(e) = store::delete_link(ctx, &identity.sub, &link.sub).await {
            tracing::error!("stale link cleanup failed: {e}");
            return reply(500, &error("internal"));
        }
        (username, created) = match first_link_native(ctx, &identity, &req).await {
            Ok(x) => x,
            Err(e) => return Ok(e),
        };
        attempt = cognito::custom_auth(ctx, &ctx.client_id, &username, &req.identity_token).await;
    }
    let tokens = match attempt {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("custom auth failed for linked user: {e}");
            return reply(500, &error("internal"));
        }
    };

    reply(
        200,
        &SignInResponse {
            id_token: tokens.id_token,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_in: tokens.expires_in,
            created,
        },
    )
}

/// The native sheet's first-use adapter over [`first_link`], turning its typed
/// error into a Function URL reply.
async fn first_link_native(
    ctx: &Arc<Ctx>,
    identity: &apple::AppleIdentity,
    req: &SignInRequest,
) -> Result<(String, bool), Value> {
    first_link(
        ctx,
        identity,
        &req.authorization_code,
        req.email.as_deref(),
        req.full_name.as_deref(),
        ctx.apple.bundle_id(),
    )
    .await
    .map_err(|(status, message)| fail(status, &message))
}

/// First use of this Apple credential: attach it to the account whose verified
/// email matches, or create a fresh account (relay emails land here by
/// construction), exchange the authorization code for the revocable Apple
/// refresh token, and write the link. Shared by the native and web flows —
/// `apple_client_id` records which one minted the code (drives the exchange and
/// is stored for revoke). Returns the Function URL error reply on failure.
pub(crate) async fn first_link(
    ctx: &Arc<Ctx>,
    identity: &apple::AppleIdentity,
    authorization_code: &str,
    email_seen: Option<&str>,
    name_seen: Option<&str>,
    apple_client_id: &str,
) -> Result<(String, bool), (u16, String)> {
    let internal = || (500u16, "internal".to_owned());
    let Some(email) = identity.email.as_deref() else {
        // No verified email in the token and no existing link: the only path
        // here is a stale prior grant. The user resets it in Settings → Sign
        // in with Apple; the client surfaces this text.
        return Err((
            400,
            "apple grant has no email; remove lux under Settings > Sign in with Apple and retry"
                .to_owned(),
        ));
    };

    let (user, created) = match cognito::find_user_by_email(ctx, email).await {
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            return Err(internal());
        }
        Ok(Some(user)) => {
            // A self-signup that never entered its confirmation code: Apple
            // just verified the same address, so confirm and proceed.
            if user.unconfirmed {
                if let Err(e) = cognito::confirm_user(ctx, &user.username).await {
                    tracing::error!("user confirm failed: {e}");
                    return Err(internal());
                }
            }
            (user, false)
        }
        Ok(None) => match cognito::create_user(ctx, email).await {
            Ok(u) => (u, true),
            Err(e) => {
                tracing::error!("user create failed: {e}");
                return Err(internal());
            }
        },
    };

    // The code exchange is required on first link: a mapping without a
    // revocable Apple token could never honor account deletion's revocation
    // duty, so fail loud instead (the client mints a fresh code on retry).
    let apple_refresh = match exchange_code(ctx, authorization_code, apple_client_id).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("apple code exchange failed: {e}");
            return Err((502, "apple token exchange failed".to_owned()));
        }
    };

    if let Err(e) = store::put_link(
        ctx,
        &identity.sub,
        &user.username,
        &user.sub,
        &apple_refresh,
        apple_client_id,
        email_seen.or(Some(email)),
        name_seen,
    )
    .await
    {
        tracing::error!("link write failed: {e}");
        return Err(internal());
    }

    Ok((user.username, created))
}

/// `POST /auth/apple/link` — bearer-authed: bind the caller's account to the
/// presented Apple credential regardless of email (the Hide My Email path).
/// Links are 1:1 — an Apple id links one account, an account links one Apple id.
async fn link(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(caller_sub) = caller(ctx, event) else {
        return reply(401, &error("invalid or missing token"));
    };
    let req: SignInRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };
    let identity = match ctx
        .apple
        .verify_identity(&req.identity_token, &req.raw_nonce)
        .await
    {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("apple identity token rejected: {e}");
            return reply(401, &error("invalid apple identity token"));
        }
    };

    match store::get_link(ctx, &identity.sub).await {
        Err(e) => {
            tracing::error!("link lookup failed: {e}");
            return reply(500, &error("internal"));
        }
        Ok(Some(link)) if link.sub == caller_sub => {
            return reply(200, &LinkResponse { linked: true })
        }
        Ok(Some(_)) => return reply(409, &error("apple id already linked to another account")),
        Ok(None) => {}
    }
    match store::get_reverse(ctx, &caller_sub).await {
        Err(e) => {
            tracing::error!("reverse link lookup failed: {e}");
            return reply(500, &error("internal"));
        }
        Ok(Some(_)) => return reply(409, &error("account already linked to an apple id")),
        Ok(None) => {}
    }

    let user = match cognito::find_user_by_sub(ctx, &caller_sub).await {
        Ok(Some(u)) => u,
        Ok(None) => return reply(404, &error("account not found")),
        Err(e) => {
            tracing::error!("user lookup failed: {e}");
            return reply(500, &error("internal"));
        }
    };

    let apple_refresh =
        match exchange_code(ctx, &req.authorization_code, ctx.apple.bundle_id()).await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("apple code exchange failed: {e}");
                return reply(502, &error("apple token exchange failed"));
            }
        };

    if let Err(e) = store::put_link(
        ctx,
        &identity.sub,
        &user.username,
        &user.sub,
        &apple_refresh,
        ctx.apple.bundle_id(),
        identity.email.as_deref(),
        req.full_name.as_deref(),
    )
    .await
    {
        tracing::error!("link write failed: {e}");
        return reply(500, &error("internal"));
    }

    reply(200, &LinkResponse { linked: true })
}

/// `POST /auth/apple/revoke` — bearer-authed: revoke the stored Apple refresh
/// token and drop the link. Account deletion calls this first; on Apple-side
/// failure the link is kept and the call is retryable.
async fn revoke(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let Some(caller_sub) = caller(ctx, event) else {
        return reply(401, &error("invalid or missing token"));
    };

    let apple_sub = match store::get_reverse(ctx, &caller_sub).await {
        Err(e) => {
            tracing::error!("reverse link lookup failed: {e}");
            return reply(500, &error("internal"));
        }
        Ok(None) => return reply(200, &RevokeResponse { revoked: false }),
        Ok(Some(s)) => s,
    };
    let link = match store::get_link(ctx, &apple_sub).await {
        Ok(Some(l)) => l,
        Ok(None) => return reply(200, &RevokeResponse { revoked: false }),
        Err(e) => {
            tracing::error!("link lookup failed: {e}");
            return reply(500, &error("internal"));
        }
    };

    // Revoke with the client that minted the stored token (bundle id for links
    // predating the field — those are all native) and the key that flow signs
    // with (native → siwa_key, web → siwa_web_key).
    let client_id = link
        .apple_client_id
        .as_deref()
        .unwrap_or_else(|| ctx.apple.bundle_id());
    let key = if client_id == ctx.apple.bundle_id() {
        ctx.siwa_key().await
    } else {
        ctx.siwa_web_key().await
    };
    let key = match key {
        Ok(k) => k,
        Err(e) => {
            tracing::error!("{e}");
            return reply(500, &error("apple signing key unavailable"));
        }
    };
    if let Err(e) = ctx
        .apple
        .revoke(key, &link.apple_refresh_token, client_id)
        .await
    {
        tracing::error!("apple revoke failed: {e}");
        return reply(502, &error("apple revoke failed"));
    }

    if let Err(e) = store::delete_link(ctx, &apple_sub, &caller_sub).await {
        // The Apple-side revoke succeeded; a dangling mapping row is only
        // local noise, but surface it — deletion flows should be silent-clean.
        tracing::error!("link delete failed after revoke: {e}");
        return reply(500, &error("internal"));
    }

    reply(200, &RevokeResponse { revoked: true })
}

// --- shared steps -------------------------------------------------------------

/// Exchange a single-use authorization code for Apple's revocable refresh token
/// (needs the Apple-side signing key). `client_id` picks the flow that minted
/// the code — the bundle id for the native sheet, the Services ID for the web
/// flow — since Apple ties the code to the `client_id` that authorized it.
pub(crate) async fn exchange_code(
    ctx: &Arc<Ctx>,
    code: &str,
    client_id: &str,
) -> Result<String, String> {
    if client_id == ctx.apple.bundle_id() {
        ctx.apple.exchange_code(ctx.siwa_key().await?, code).await
    } else {
        ctx.apple
            .exchange_code_web(ctx.siwa_web_key().await?, code)
            .await
    }
}

/// Best-effort on re-auth: keep the stored revocable token (and the `client_id`
/// that minted it) fresh.
async fn refresh_apple_token(
    ctx: &Arc<Ctx>,
    apple_sub: &str,
    code: &str,
    client_id: &str,
) -> Result<(), String> {
    let token = exchange_code(ctx, code, client_id).await?;
    store::set_refresh_token(ctx, apple_sub, &token, client_id).await
}

// --- request/response helpers -------------------------------------------------

/// The verified caller (`sub`) from the bearer token, if any.
pub(crate) fn caller(ctx: &Ctx, event: &UrlEvent) -> Option<String> {
    let token = event
        .headers
        .get("authorization")?
        .strip_prefix("Bearer ")?;
    ctx.verifier.verify(token).ok().map(|c| c.sub)
}

/// The request body, base64-decoded if the Function URL flagged it (binary
/// bodies — and Apple's `application/x-www-form-urlencoded` callback POST —
/// arrive base64-encoded).
pub(crate) fn body_bytes(event: &UrlEvent) -> Result<Vec<u8>, String> {
    let raw = event.body.as_deref().unwrap_or_default();
    if event.is_base64_encoded {
        BASE64
            .decode(raw)
            .map_err(|e| format!("bad body encoding: {e}"))
    } else {
        Ok(raw.as_bytes().to_vec())
    }
}

pub(crate) fn parse_body<T: for<'de> Deserialize<'de>>(event: &UrlEvent) -> Result<T, String> {
    serde_json::from_slice(&body_bytes(event)?).map_err(|e| format!("bad body: {e}"))
}

pub(crate) fn error(message: &str) -> ErrorResponse {
    ErrorResponse {
        error: message.to_owned(),
    }
}

/// A Function URL (payload v2) response.
pub(crate) fn reply<T: serde::Serialize>(status: u16, body: &T) -> Result<Value, Error> {
    Ok(json!({
        "statusCode": status,
        "headers": { "content-type": "application/json" },
        "body": serde_json::to_string(body)?,
    }))
}

/// A 302 redirect (the web callback's only response shape — it sends the
/// browser back to the app's `lux://` scheme, carrying a one-time code or an
/// error, never a token).
pub(crate) fn redirect(location: &str) -> Result<Value, Error> {
    Ok(json!({
        "statusCode": 302,
        "headers": { "location": location },
        "body": "",
    }))
}

/// Same as [`reply`] but usable where a `Value` (not a `Result`) is needed.
fn fail(status: u16, message: &str) -> Value {
    json!({
        "statusCode": status,
        "headers": { "content-type": "application/json" },
        "body": serde_json::to_string(&error(message)).unwrap_or_else(|_| "{}".into()),
    })
}
