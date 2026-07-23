//! Web (browser) Sign in with Apple — the `.dmg`/dev fallback where the native
//! sheet is impossible (Apple forbids the applesignin entitlement off the Mac
//! App Store). Design + rationale: `.claude/specs/sign-in-with-apple-web.md`.
//!
//! The service owns all Apple config; the app holds only a PKCE secret. Three
//! routes thread one flow:
//! - `POST /auth/apple/web/start`    — mint the authorize URL (server-chosen
//!   `state` + `nonce`, the app's PKCE `code_challenge` banked against `state`).
//! - `POST /auth/apple/web/callback` — Apple's `form_post` target: verify the
//!   id_token, resolve/link the user, stash a one-time code, 302 the browser
//!   back to `lux://` (only the opaque code + `state` ride on it, never a token).
//! - `POST /auth/apple/web/exchange` — trade the one-time code (+ PKCE verifier)
//!   for the same Cognito tokens the native and SRP paths return.
//!
//! Everything is dark until the Services ID (`apple`) and its redirect
//! (`apple_web_callback_url`) are both configured — `/start` 404s without them.

use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use lambda_runtime::Error;
use lux_wire::apple::{
    SignInResponse, WebExchangeRequest, WebStartRequest, WebStartResponse, WEB_REDIRECT_URL,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::http::{body_bytes, error, parse_body, redirect, reply, UrlEvent};
use crate::{cognito, http, store, Ctx};

const APPLE_AUTHORIZE_URL: &str = "https://appleid.apple.com/auth/authorize";
/// The browser-auth window — the user has this long to finish at Apple.
const STATE_TTL_SECS: i64 = 600;
/// The app exchanges within seconds of the redirect; keep the code brief.
const OTC_TTL_SECS: i64 = 120;

/// `POST /auth/apple/web/start` — bank the PKCE challenge and hand back the
/// Apple authorize URL to open in `ASWebAuthenticationSession`.
pub async fn start(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let (Some(services_id), Some(callback)) = (
        ctx.apple.services_id(),
        ctx.apple_web_callback_url.as_deref(),
    ) else {
        return reply(404, &error("web sign-in not configured"));
    };
    let req: WebStartRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };
    if req.code_challenge.is_empty() {
        return reply(400, &error("missing code challenge"));
    }

    let (state, nonce) = match (rand_token(), rand_token()) {
        (Ok(s), Ok(n)) => (s, n),
        _ => {
            tracing::error!("web start: no OS randomness");
            return reply(500, &error("internal"));
        }
    };
    if let Err(e) =
        store::put_web_state(ctx, &state, &nonce, &req.code_challenge, STATE_TTL_SECS).await
    {
        tracing::error!("web start state write failed: {e}");
        return reply(500, &error("internal"));
    }

    reply(
        200,
        &WebStartResponse {
            authorize_url: authorize_url(services_id, callback, &state, &nonce),
        },
    )
}

/// `POST /auth/apple/web/callback` — Apple's `form_post` lands here. Always
/// answers with a 302 back to `lux://` (code on success, `error` otherwise) so
/// the app's auth session always completes rather than hanging.
pub async fn callback(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let bytes = match body_bytes(event) {
        Ok(b) => b,
        Err(_) => return redirect(&err_redirect("", "bad callback body")),
    };
    let form = parse_form(&String::from_utf8_lossy(&bytes));
    let state = form.get("state").cloned().unwrap_or_default();

    // Apple can hand back its own error (user cancelled / denied).
    if let Some(e) = form.get("error") {
        return redirect(&err_redirect(&state, e));
    }
    let (Some(id_token), Some(code)) = (form.get("id_token"), form.get("code")) else {
        return redirect(&err_redirect(&state, "missing code or id_token"));
    };

    // Consume the state once — yields the nonce we chose and the PKCE challenge.
    let web_state = match store::take_web_state(ctx, &state).await {
        Ok(Some(s)) => s,
        Ok(None) => return redirect(&err_redirect(&state, "unknown or expired state")),
        Err(e) => {
            tracing::error!("web state take failed: {e}");
            return redirect(&err_redirect(&state, "internal"));
        }
    };

    let identity = match ctx
        .apple
        .verify_identity_web(id_token, &web_state.nonce)
        .await
    {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("web id_token rejected: {e}");
            return redirect(&err_redirect(&state, "invalid apple token"));
        }
    };
    let Some(services_id) = ctx.apple.services_id() else {
        return redirect(&err_redirect(&state, "web sign-in not configured"));
    };

    // First-authorization display data (name/email) rides in `user`, once.
    let (email_seen, name_seen) = user_fields(form.get("user").map(String::as_str));

    let (username, created) = match store::get_link(ctx, &identity.sub).await {
        Err(e) => {
            tracing::error!("link lookup failed: {e}");
            return redirect(&err_redirect(&state, "internal"));
        }
        Ok(Some(link)) => {
            // Known Apple user — keep the revocable token fresh, best-effort.
            if let Ok(token) = http::exchange_code(ctx, code, services_id).await {
                let _ = store::set_refresh_token(ctx, &identity.sub, &token, services_id).await;
            }
            (link.username, false)
        }
        Ok(None) => match http::first_link(
            ctx,
            &identity,
            code,
            email_seen.as_deref(),
            name_seen.as_deref(),
            services_id,
        )
        .await
        {
            Ok(x) => x,
            Err((_, msg)) => return redirect(&err_redirect(&state, &msg)),
        },
    };

    // Stash a one-time code (holding the verified id_token as the CUSTOM_AUTH
    // answer) and send the browser back to the app.
    let otc = match rand_token() {
        Ok(o) => o,
        Err(_) => return redirect(&err_redirect(&state, "internal")),
    };
    if let Err(e) = store::put_web_otc(
        ctx,
        &otc,
        &username,
        id_token,
        created,
        &web_state.code_challenge,
        OTC_TTL_SECS,
    )
    .await
    {
        tracing::error!("web otc write failed: {e}");
        return redirect(&err_redirect(&state, "internal"));
    }

    redirect(&ok_redirect(&state, &otc))
}

/// `POST /auth/apple/web/exchange` — trade the one-time code (+ PKCE verifier)
/// for Cognito tokens.
pub async fn exchange(ctx: &Arc<Ctx>, event: &UrlEvent) -> Result<Value, Error> {
    let req: WebExchangeRequest = match parse_body(event) {
        Ok(b) => b,
        Err(e) => return reply(400, &error(&e)),
    };

    let otc = match store::take_web_otc(ctx, &req.code).await {
        Ok(Some(o)) => o,
        Ok(None) => return reply(400, &error("unknown or expired code")),
        Err(e) => {
            tracing::error!("web otc take failed: {e}");
            return reply(500, &error("internal"));
        }
    };

    // PKCE: only the app instance that started the flow holds the verifier whose
    // challenge we banked — a rogue app that grabbed the `lux://` redirect can't.
    if pkce_challenge(&req.code_verifier) != otc.code_challenge {
        tracing::warn!("web exchange pkce mismatch");
        return reply(400, &error("code verification failed"));
    }

    let tokens = match cognito::custom_auth(ctx, &ctx.client_id, &otc.username, &otc.id_token).await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("web custom auth failed: {e}");
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
            created: otc.created,
        },
    )
}

// --- helpers -----------------------------------------------------------------

/// The Apple `/auth/authorize` URL. `response_mode=form_post` is mandatory once
/// `scope` carries name/email (Apple rejects query mode there), which is why the
/// callback is a POST-capable https endpoint.
fn authorize_url(client_id: &str, redirect_uri: &str, state: &str, nonce: &str) -> String {
    let query = [
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code id_token"),
        ("response_mode", "form_post"),
        ("scope", "name email"),
        ("state", state),
        ("nonce", nonce),
    ]
    .iter()
    .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{APPLE_AUTHORIZE_URL}?{query}")
}

fn ok_redirect(state: &str, code: &str) -> String {
    format!("{WEB_REDIRECT_URL}?code={}&state={}", enc(code), enc(state))
}

fn err_redirect(state: &str, message: &str) -> String {
    format!(
        "{WEB_REDIRECT_URL}?error={}&state={}",
        enc(message),
        enc(state)
    )
}

/// PKCE `S256`: `base64url(SHA-256(verifier))`, no padding (RFC 7636).
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// 32 bytes of OS randomness, base64url — opaque and unguessable. Returns an
/// error rather than a weak fallback, so a randomness failure is a 500, never a
/// guessable state/nonce/code.
fn rand_token() -> Result<String, String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no OS randomness: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Pull the once-only `email`/`name` out of Apple's `user` JSON, if present.
fn user_fields(user: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = user else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return (None, None);
    };
    let email = v.get("email").and_then(Value::as_str).map(str::to_owned);
    let name = v.get("name").and_then(|n| {
        let first = n.get("firstName").and_then(Value::as_str).unwrap_or("");
        let last = n.get("lastName").and_then(Value::as_str).unwrap_or("");
        let joined = format!("{first} {last}");
        let trimmed = joined.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    });
    (email, name)
}

/// Parse an `application/x-www-form-urlencoded` body into a map (last value
/// wins on a repeated key — no such key exists in Apple's callback).
fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let mut it = pair.splitn(2, '=');
            let key = it.next()?;
            let value = it.next().unwrap_or("");
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

/// Percent-decode a form field (`+` is a space, `%XX` a byte). Invalid escapes
/// pass through literally rather than erroring — a callback field is never
/// re-emitted, only read.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-encode for a URL query (RFC 3986 unreserved pass through; everything
/// else — spaces in `scope`/`response_type`, `:`/`/` in the redirect — is
/// `%XX`).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_test_vector() {
        // RFC 7636 Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_has_form_post_and_encoded_params() {
        // Use a real generated nonce, not a literal: the test only checks the
        // URL's structure/encoding (never the nonce value), and a hard-coded
        // crypto value trips CodeQL for no reason.
        let nonce = rand_token().expect("randomness");
        let url = authorize_url(
            "com.johncarmack.lux.signin",
            "https://auth.lux.johncarmack.com/auth/apple/web/callback",
            "st ate",
            &nonce,
        );
        assert!(url.starts_with("https://appleid.apple.com/auth/authorize?"));
        assert!(url.contains("client_id=com.johncarmack.lux.signin"));
        assert!(url.contains("response_mode=form_post"));
        // The response_type is the two space-separated tokens "code id_token".
        assert!(url.contains("response_type=code%20id_token"));
        assert!(url.contains("scope=name%20email"));
        // redirect and state are percent-encoded.
        assert!(url.contains("redirect_uri=https%3A%2F%2Fauth.lux.johncarmack.com"));
        assert!(url.contains("state=st%20ate"));
    }

    #[test]
    fn redirects_are_tokenless_and_scheme_bound() {
        let ok = ok_redirect("s1", "otc-code");
        assert_eq!(ok, "lux://auth/apple/callback?code=otc-code&state=s1");
        let err = err_redirect("s 1", "invalid apple token");
        assert_eq!(
            err,
            "lux://auth/apple/callback?error=invalid%20apple%20token&state=s%201"
        );
    }

    #[test]
    fn parses_apple_form_post() {
        // id_token/code shortened; `user` is JSON, url-encoded, first-auth only.
        let body = "state=abc&code=c1&id_token=eyJ.a.b&user=%7B%22name%22%3A%7B%22firstName%22%3A%22Ada%22%2C%22lastName%22%3A%22Lovelace%22%7D%2C%22email%22%3A%22ada%40example.com%22%7D";
        let form = parse_form(body);
        assert_eq!(form.get("state").map(String::as_str), Some("abc"));
        assert_eq!(form.get("code").map(String::as_str), Some("c1"));
        assert_eq!(form.get("id_token").map(String::as_str), Some("eyJ.a.b"));

        let (email, name) = user_fields(form.get("user").map(String::as_str));
        assert_eq!(email.as_deref(), Some("ada@example.com"));
        assert_eq!(name.as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn user_fields_absent_or_partial() {
        assert_eq!(user_fields(None), (None, None));
        assert_eq!(user_fields(Some("not json")), (None, None));
        // Repeat auth: no `user`, so nothing to record.
        let (email, name) = user_fields(Some(r#"{"email":"x@y.z"}"#));
        assert_eq!(email.as_deref(), Some("x@y.z"));
        assert_eq!(name, None);
    }

    #[test]
    fn rand_token_is_urlsafe_and_unique() {
        let a = rand_token().expect("randomness");
        let b = rand_token().expect("randomness");
        assert_ne!(a, b);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    }
}
