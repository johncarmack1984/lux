//! Change-nudge listener: one open MQTT-over-WebSocket connection to AWS IoT
//! Core, so this device pulls immediately when another of the user's devices
//! writes a setup — vegify's sync model (server-authoritative, nudged pull),
//! with IoT Core standing in for vegify's standing server as the connection
//! holder.
//!
//! The sync-api publishes a tiny frame to `lux/sync/user/<sub>` after each
//! committed write (`lux_wire::nudge`). The frame is opaque by design and never
//! parsed here: **any** frame means "pull now", and the pull
//! ([`crate::cloud::schedule_sync`], already single-flight) stays the
//! authoritative sync. Missed frames are healed by the existing safety nets —
//! pull on sign-in/startup/focus plus the on-(re)connect pull below — so
//! delivery is deliberately best-effort (QoS 0).
//!
//! Auth mirrors the sync-api's posture: the handshake carries the Cognito ID
//! token in the `x-lux-token` header; the `lux-sync-auth` IoT custom authorizer
//! verifies it and scopes the connection's policy to the *verified* user's own
//! topic. The token is re-read (and on auth failures refreshed) on every
//! reconnect attempt, vegify-style, with capped exponential backoff (1s→30s).
//!
//! Endpoint config follows the remote.rs contract — env-configured, else no-op
//! (`LUX_NUDGE_ENDPOINT` / `LUX_NUDGE_AUTHORIZER`; a baked production default
//! can land in `DEFAULT_NUDGE_ENDPOINT` once read from `terraform output
//! iot_endpoint`). Runs while signed in; [`stop`] on sign-out.

use std::time::Duration;

use base64::Engine;
use rumqttc::{
    AsyncClient, ConnectionError, Event, MqttOptions, Packet, QoS, TlsConfiguration, Transport,
};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;
use uuid::Uuid;

use crate::account::{env_or, webpki_pem_bundle, LuxAccount};

/// Production IoT data endpoint (`terraform output iot_endpoint`). Not a secret
/// (same class as the baked Cognito/sync constants in account.rs — the
/// authorizer only honors verified tokens). Empty = nudges disabled unless the
/// env var is set.
const DEFAULT_NUDGE_ENDPOINT: &str = "";
/// Name of the IoT custom authorizer registered in infra/nudge.tf.
const DEFAULT_NUDGE_AUTHORIZER: &str = "lux-sync-auth";

struct Config {
    endpoint: String,
    authorizer: String,
}

fn load_config() -> Option<Config> {
    let _ = dotenvy::dotenv();
    let endpoint = env_or("LUX_NUDGE_ENDPOINT", DEFAULT_NUDGE_ENDPOINT);
    if endpoint.is_empty() {
        return None;
    }
    Some(Config {
        endpoint,
        authorizer: env_or("LUX_NUDGE_AUTHORIZER", DEFAULT_NUDGE_AUTHORIZER),
    })
}

/// Tauri-managed listener lifecycle. The watch value is a generation counter:
/// [`start`] bumps it and spawns a listener bound to the new generation, so a
/// stale listener (previous sign-in, or a sign-out via [`stop`]) sees the bump
/// and exits — at most one listener is ever live.
pub struct LuxNudge {
    generation: watch::Sender<u64>,
}

impl Default for LuxNudge {
    fn default() -> Self {
        Self {
            generation: watch::channel(0).0,
        }
    }
}

/// Start (or restart) the nudge listener for the signed-in user. Called after
/// sign-in and after a startup session restore; no-op when nudges aren't
/// configured or nobody is signed in.
pub fn start(app: &AppHandle) {
    let Some(cfg) = load_config() else {
        log::info!("nudge channel not configured; realtime sync nudges disabled (set LUX_NUDGE_ENDPOINT to enable)");
        return;
    };
    if !app.state::<LuxAccount>().signed_in() {
        return;
    }

    let state = app.state::<LuxNudge>();
    let mut generation = state.generation.subscribe();
    state.generation.send_modify(|g| *g += 1);
    let my_generation = *generation.borrow_and_update();

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut backoff_secs = 1u64;
        let mut refresh_first = false;
        loop {
            if *generation.borrow() != my_generation {
                return; // superseded by a newer listener or a sign-out
            }
            let account = app.state::<LuxAccount>();
            if !account.signed_in() {
                return;
            }
            // Fresh token every attempt (vegify's contract); after an auth-shaped
            // failure, refresh it first — the previous one likely expired.
            let token = if refresh_first {
                account
                    .refresh_id_token()
                    .await
                    .ok()
                    .or_else(|| account.current_id_token())
            } else {
                account.current_id_token()
            };
            drop(account);
            let Some(token) = token else { return };
            let Some(sub) = jwt_sub(&token) else {
                log::warn!(
                    "nudge: could not read sub from the id token; disabling for this session"
                );
                return;
            };
            refresh_first =
                run_connection(&app, &cfg, token, &sub, &mut generation, &mut backoff_secs).await;
            if *generation.borrow() != my_generation {
                return;
            }
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(30);
        }
    });
}

/// Stop the listener (sign-out). Idempotent; a later [`start`] resumes.
pub fn stop(app: &AppHandle) {
    app.state::<LuxNudge>().generation.send_modify(|g| *g += 1);
}

/// One connection lifetime: connect, subscribe, forward frames to the sync
/// engine until the connection drops or the listener is superseded. Returns
/// whether the failure looked auth-shaped (broker refused the connect), so the
/// caller refreshes the token before retrying.
async fn run_connection(
    app: &AppHandle,
    cfg: &Config,
    token: String,
    sub: &str,
    generation: &mut watch::Receiver<u64>,
    backoff_secs: &mut u64,
) -> bool {
    // Random per-session suffix: one user's devices must not share a client id
    // (IoT disconnects duplicates). The authorizer's policy allows the prefix.
    let client_id = format!(
        "{}{}",
        lux_wire::nudge::client_id_prefix(sub),
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let url = format!(
        "wss://{}/mqtt?x-amz-customauthorizer-name={}",
        cfg.endpoint, cfg.authorizer
    );
    let mut opts = MqttOptions::new(client_id, url, 443);
    opts.set_keep_alive(Duration::from_secs(30));
    // Trust the bundled webpki roots, not the platform store (unreadable on
    // iOS — same lesson as the account layer's AWS SDK client).
    opts.set_transport(Transport::Wss(TlsConfiguration::Simple {
        ca: webpki_pem_bundle().to_vec(),
        alpn: None,
        client_auth: None,
    }));
    let header_token = token.clone();
    opts.set_request_modifier(move |mut request| {
        let value = header_token.clone();
        async move {
            if let Ok(v) = value.parse() {
                request.headers_mut().insert(lux_wire::nudge::TOKEN_KEY, v);
            }
            request
        }
    });

    let (client, mut eventloop) = AsyncClient::new(opts, 10);
    if let Err(e) = client
        .subscribe(lux_wire::nudge::user_topic(sub), QoS::AtMostOnce)
        .await
    {
        log::warn!("nudge: could not queue subscribe: {e}");
        return false;
    }

    loop {
        tokio::select! {
            _ = generation.changed() => {
                let _ = client.disconnect().await;
                return false; // superseded — the outer loop exits
            }
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    log::info!("nudge channel connected; listening for setup changes");
                    *backoff_secs = 1;
                    // On-(re)connect pull: cover anything nudged while offline.
                    crate::cloud::schedule_sync(app);
                }
                Ok(Event::Incoming(Packet::Publish(_))) => {
                    // Opaque frame — never parsed; any frame means "pull now".
                    log::debug!("nudge received; scheduling sync");
                    crate::cloud::schedule_sync(app);
                }
                Ok(_) => {}
                Err(e) => {
                    log::info!("nudge connection error (will reconnect): {e}");
                    return matches!(e, ConnectionError::ConnectionRefused(_));
                }
            }
        }
    }
}

/// The `sub` claim from our own ID token's payload. Unverified base64 decode on
/// purpose: this only *addresses* the topic we ask for — the IoT authorizer
/// independently verifies the token and scopes the granted policy to the sub it
/// verified, so a wrong value here can only produce a denied subscribe.
fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(claims.get("sub")?.as_str()?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_sub_reads_the_payload_claim() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"sub":"abc-123","token_use":"id"}"#);
        let token = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
        assert_eq!(jwt_sub(&token).as_deref(), Some("abc-123"));
    }

    #[test]
    fn jwt_sub_rejects_garbage() {
        assert_eq!(jwt_sub("not-a-jwt"), None);
        assert_eq!(jwt_sub("a.b.c"), None);
    }
}
