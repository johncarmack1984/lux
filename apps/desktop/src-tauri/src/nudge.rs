//! The user channel: one open MQTT-over-WebSocket connection to AWS IoT Core
//! per signed-in device, carrying two kinds of traffic routed by topic:
//!
//! - **Change nudges** (`lux/sync/user/<sub>`) — the sync-api publishes a tiny
//!   frame after each committed write. The frame is opaque by design and never
//!   parsed here: **any** frame means "pull now", and the pull
//!   ([`crate::cloud::schedule_sync`], already single-flight) stays the
//!   authoritative sync. Missed frames are healed by the existing safety nets —
//!   pull on sign-in/startup/focus plus the on-(re)connect pull below — so
//!   delivery is deliberately best-effort (QoS 0).
//! - **Remote control** (`lux/ctl/user/<sub>/…`, `lux_wire::ctl`) — live
//!   buffer frames published by the user's other surfaces. Frames addressed to
//!   this device's *active* setup run through the same [`LuxBuffer`] paths as
//!   local input (overlay/channel semantics preserved); everything else is
//!   dropped. The connection also announces itself with a retained presence
//!   card (cleared by the Last Will on ungraceful drops and explicitly on
//!   sign-out) and keeps a retained state echo — the last-applied full buffer,
//!   coalesced to ≤5 Hz — so remote surfaces can reflect truth, including
//!   changes made locally at this device.
//!
//! Auth mirrors the sync-api's posture: the handshake carries the Cognito ID
//! token in the `x-lux-token` header; the `lux-sync-auth` IoT custom authorizer
//! verifies it and scopes the connection's policy to the *verified* user's own
//! topics. The token is re-read (and on auth failures refreshed) on every
//! reconnect attempt, vegify-style, with capped exponential backoff (1s→30s).
//!
//! The IoT endpoint comes from [`crate::endpoints`] (the generated-and-embedded
//! production config, `endpoints.local.json` for dev stacks) — missing means
//! the channel is off and pull-based sync carries everything. The authorizer
//! name is protocol, not environment (`lux_wire::nudge::AUTHORIZER_NAME`). Runs
//! while signed in; [`stop`] on sign-out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use rumqttc::{
    AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Packet, QoS, SubscribeFilter,
    TlsConfiguration, Transport,
};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::watch;
use uuid::Uuid;

use crate::account::{webpki_pem_bundle, LuxAccount};
use crate::buffer::LuxBuffer;
use crate::lock::LockPolicy;
use crate::setup::LuxSetups;

/// Trailing-edge coalescing window for the retained state echo (≤5 Hz) — a
/// remote surface needs truth, not every intermediate slider position.
const ECHO_WINDOW: Duration = Duration::from_millis(200);

fn nudge_endpoint() -> Option<String> {
    Some(crate::endpoints::effective().nudge_endpoint.clone()).filter(|e| !e.is_empty())
}

/// Tauri-managed listener lifecycle. The `generation` watch value is a
/// counter: [`start`] bumps it and spawns a listener bound to the new
/// generation, so a stale listener (previous sign-in, or a sign-out via
/// [`stop`]) sees the bump and exits — at most one listener is ever live.
/// `presence` is bumped by [`presence_changed`] so the live connection
/// republishes its card without reconnecting; `echo` is the live connection's
/// publish handle for [`schedule_state_echo`].
pub struct LuxNudge {
    generation: watch::Sender<u64>,
    presence: watch::Sender<u64>,
    echo: Mutex<Option<EchoHandle>>,
    echo_pending: AtomicBool,
}

impl Default for LuxNudge {
    fn default() -> Self {
        Self {
            generation: watch::channel(0).0,
            presence: watch::channel(0).0,
            echo: Mutex::new(None),
            echo_pending: AtomicBool::new(false),
        }
    }
}

impl LuxNudge {
    fn set_echo(&self, handle: EchoHandle) {
        *self.echo.lock_or_recover() = Some(handle);
    }

    /// Clear the echo handle, but only if it still belongs to `session` — a
    /// replacement connection may already have installed its own.
    fn clear_echo(&self, session: &str) {
        let mut echo = self.echo.lock_or_recover();
        if echo.as_ref().is_some_and(|e| e.session == session) {
            *echo = None;
        }
    }
}

/// The live connection's handle for publishing the retained state echo.
#[derive(Clone)]
struct EchoHandle {
    client: AsyncClient,
    sub: String,
    session: String,
}

/// Start (or restart) the listener for the signed-in user. Called after
/// sign-in and after a startup session restore; no-op when the channel isn't
/// configured or nobody is signed in.
pub fn start(app: &AppHandle) {
    let Some(endpoint) = nudge_endpoint() else {
        log::info!("nudge endpoint not configured (endpoints file); user channel disabled");
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
            let Some(token) = token else { return };
            let Some(sub) = jwt_sub(&token) else {
                log::warn!(
                    "nudge: could not read sub from the id token; disabling for this session"
                );
                return;
            };
            refresh_first = run_connection(
                &app,
                &endpoint,
                token,
                &sub,
                &mut generation,
                &mut backoff_secs,
            )
            .await;
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

/// Ask the live connection to republish its presence card — called when the
/// active setup changes, so remote surfaces learn the new binding without a
/// reconnect. No-op when no connection is up.
pub fn presence_changed<R: Runtime>(app: &AppHandle<R>) {
    app.state::<LuxNudge>().presence.send_modify(|g| *g += 1);
}

/// Schedule a retained state-echo publish of the current buffer to the active
/// setup's `state` topic, trailing-edge coalesced to [`ECHO_WINDOW`]. Called
/// from the buffer's commit path, so local input and remotely-applied frames
/// both refresh the echo; fast no-op when no connection is live. The echo is
/// retained-topic-only and never fed back into the buffer, so it cannot loop.
pub fn schedule_state_echo<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LuxNudge>();
    if state.echo.lock_or_recover().is_none() {
        return;
    }
    if state.echo_pending.swap(true, Ordering::SeqCst) {
        return; // a publish is already queued and will pick this change up
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(ECHO_WINDOW).await;
        let state = app.state::<LuxNudge>();
        state.echo_pending.store(false, Ordering::SeqCst);
        let Some(echo) = state.echo.lock_or_recover().clone() else {
            return;
        };
        let buffer: Vec<u8> = app.state::<LuxBuffer>().buffer.lock_or_recover().clone();
        let setup_id = app.state::<LuxSetups>().active_id().to_string();
        let frame = lux_wire::ctl::Frame::buffer(buffer).with_src(&echo.session);
        let Ok(payload) = serde_json::to_vec(&frame) else {
            return;
        };
        let topic = lux_wire::ctl::state_topic(&echo.sub, &setup_id);
        if let Err(e) = echo
            .client
            .publish(topic, QoS::AtMostOnce, true, payload)
            .await
        {
            log::debug!("state echo publish failed (connection likely down): {e}");
        }
    });
}

/// One connection lifetime: connect, subscribe (nudge + ctl), announce
/// presence, then route incoming traffic until the connection drops or the
/// listener is superseded. Returns whether the failure looked auth-shaped
/// (broker refused the connect), so the caller refreshes the token before
/// retrying.
async fn run_connection(
    app: &AppHandle,
    endpoint: &str,
    token: String,
    sub: &str,
    generation: &mut watch::Receiver<u64>,
    backoff_secs: &mut u64,
) -> bool {
    // Random per-session suffix: one user's devices must not share a client id
    // (IoT disconnects duplicates). The authorizer's policy allows the prefix.
    // The suffix doubles as the connection's ctl session id — it names the
    // presence topic and stamps outgoing frames (`Frame::src`).
    let session = Uuid::new_v4().simple().to_string()[..8].to_owned();
    let client_id = format!("{}{}", lux_wire::nudge::client_id_prefix(sub), session);
    let presence_topic = lux_wire::ctl::presence_topic(sub, &session);
    let url = format!(
        "wss://{endpoint}/mqtt?x-amz-customauthorizer-name={}",
        lux_wire::nudge::AUTHORIZER_NAME
    );
    let mut opts = MqttOptions::new(client_id, url, 443);
    opts.set_keep_alive(Duration::from_secs(30));
    // Ungraceful drops clear our retained presence card (empty retained
    // payload = delete); the graceful paths below publish the same goodbye.
    opts.set_last_will(LastWill::new(
        presence_topic.clone(),
        Vec::<u8>::new(),
        QoS::AtMostOnce,
        true,
    ));
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
        .subscribe_many([
            SubscribeFilter::new(lux_wire::nudge::user_topic(sub), QoS::AtMostOnce),
            SubscribeFilter::new(lux_wire::ctl::user_filter(sub), QoS::AtMostOnce),
        ])
        .await
    {
        log::warn!("nudge: could not queue subscribe: {e}");
        return false;
    }

    let state = app.state::<LuxNudge>();
    let mut presence_rx = state.presence.subscribe();
    presence_rx.borrow_and_update();

    loop {
        tokio::select! {
            _ = generation.changed() => {
                // Graceful goodbye: clear the retained presence card so other
                // surfaces grey out immediately (the Last Will only fires on
                // ungraceful drops).
                let _ = client
                    .publish(presence_topic.clone(), QoS::AtMostOnce, true, Vec::<u8>::new())
                    .await;
                let _ = client.disconnect().await;
                state.clear_echo(&session);
                return false; // superseded — the outer loop exits
            }
            _ = presence_rx.changed() => {
                publish_presence(&client, app, sub, &session).await;
            }
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    log::info!("user channel connected; nudges + remote control live");
                    *backoff_secs = 1;
                    state.set_echo(EchoHandle {
                        client: client.clone(),
                        sub: sub.to_owned(),
                        session: session.clone(),
                    });
                    publish_presence(&client, app, sub, &session).await;
                    // Refresh the retained truth for whoever is watching.
                    schedule_state_echo(app);
                    // On-(re)connect pull: cover anything nudged while offline.
                    crate::cloud::schedule_sync(app);
                }
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    match route(&publish.topic, sub) {
                        Route::Nudge => {
                            // Opaque frame — never parsed; any frame means "pull now".
                            log::debug!("nudge received; scheduling sync");
                            crate::cloud::schedule_sync(app);
                        }
                        Route::Frame { setup_id } => {
                            apply_frame(app, &publish.payload, setup_id, &session);
                        }
                        Route::Passive => {}
                        Route::Unknown => {
                            log::debug!("ignoring publish on unexpected topic {}", publish.topic);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    log::info!("nudge connection error (will reconnect): {e}");
                    state.clear_echo(&session);
                    return matches!(e, ConnectionError::ConnectionRefused(_));
                }
            }
        }
    }
}

/// Publish (or refresh) this connection's retained presence card.
async fn publish_presence(client: &AsyncClient, app: &AppHandle, sub: &str, session: &str) {
    let setup_id = app.state::<LuxSetups>().active_id().to_string();
    let card = lux_wire::ctl::PresenceCard::new(session.to_owned(), setup_id, device_name());
    let Ok(payload) = serde_json::to_vec(&card) else {
        return;
    };
    let topic = lux_wire::ctl::presence_topic(sub, session);
    if let Err(e) = client.publish(topic, QoS::AtMostOnce, true, payload).await {
        log::debug!("presence publish failed: {e}");
    }
}

/// This device's human-readable name for presence cards.
fn device_name() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// Parse a ctl frame and, if the gate lets it through, run it down the same
/// buffer paths local input uses — so a remote write behaves exactly like a
/// local one (overlay semantics, BufferSet emission, persistence, render).
fn apply_frame(app: &AppHandle, payload: &[u8], frame_setup: &str, own_session: &str) {
    let frame: lux_wire::ctl::Frame = match serde_json::from_slice(payload) {
        Ok(frame) => frame,
        Err(e) => {
            log::warn!("ignoring unreadable ctl frame: {e}");
            return;
        }
    };
    let active = app.state::<LuxSetups>().active_id().to_string();
    let Some(apply) = gate(frame, frame_setup, &active, own_session) else {
        return;
    };
    let mut buffer = app.state::<LuxBuffer>().inner().clone();
    let result = match apply {
        RemoteApply::Overlay(bytes) => buffer.set(bytes, app.clone()).map(|_| ()),
        RemoteApply::Channel { ch, val } => buffer
            .set_channel(usize::from(ch), val, app.clone())
            .map(|_| ()),
    };
    if let Err(e) = result {
        log::warn!("ctl frame apply failed: {e}");
    }
}

/// Where an incoming publish on the user channel goes.
#[derive(Debug, PartialEq, Eq)]
enum Route<'t> {
    /// The opaque nudge topic — schedule a pull.
    Nudge,
    /// A live ctl frame addressed to one setup.
    Frame { setup_id: &'t str },
    /// Retained ctl traffic (presence cards, state echoes, reserved config) —
    /// published by peers including this device; nothing for the listener to
    /// do with it today.
    Passive,
    /// Not a topic this listener expects under the granted policy.
    Unknown,
}

/// Classify an incoming topic. Plain function over plain types on purpose —
/// the routing decision must stay testable (and extractable) without Tauri.
fn route<'t>(topic: &'t str, sub: &str) -> Route<'t> {
    if topic == lux_wire::nudge::user_topic(sub) {
        return Route::Nudge;
    }
    let prefix = lux_wire::ctl::user_prefix(sub);
    let Some(rest) = topic
        .strip_prefix(prefix.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return Route::Unknown;
    };
    if let Some(setup_rest) = rest.strip_prefix("setup/") {
        return match setup_rest.split_once('/') {
            Some((setup_id, "frame")) => Route::Frame { setup_id },
            Some((_, "state" | "config")) => Route::Passive,
            _ => Route::Unknown,
        };
    }
    if rest.strip_prefix("presence/").is_some() {
        return Route::Passive;
    }
    Route::Unknown
}

/// One applicable buffer mutation extracted from a gated ctl frame.
#[derive(Debug, PartialEq, Eq)]
enum RemoteApply {
    Overlay(Vec<u8>),
    Channel { ch: u16, val: u8 },
}

/// Whether this peer applies `frame`: the version must be known, the frame
/// must not be this connection's own publish echoed back (`src` == our
/// session), and only frames addressed to the active setup apply. Plain
/// function on purpose (see [`route`]).
fn gate(
    frame: lux_wire::ctl::Frame,
    frame_setup: &str,
    active_setup: &str,
    own_session: &str,
) -> Option<RemoteApply> {
    if frame.version() != lux_wire::ctl::VERSION {
        log::debug!(
            "dropping ctl frame with unknown version {}",
            frame.version()
        );
        return None;
    }
    if frame.src() == Some(own_session) {
        return None; // our own frame, already applied locally
    }
    if frame_setup != active_setup {
        log::debug!("dropping ctl frame for inactive setup {frame_setup}");
        return None;
    }
    match frame {
        lux_wire::ctl::Frame::Buffer { buffer, .. } => Some(RemoteApply::Overlay(buffer)),
        lux_wire::ctl::Frame::Channel { ch, val, .. } => Some(RemoteApply::Channel { ch, val }),
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
    use lux_wire::ctl::Frame;

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

    #[test]
    fn route_classifies_the_user_channel() {
        let sub = "abc-123";
        assert_eq!(route("lux/sync/user/abc-123", sub), Route::Nudge);
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/frame", sub),
            Route::Frame { setup_id: "s-1" }
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/state", sub),
            Route::Passive
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/config", sub),
            Route::Passive
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/presence/0a1b2c3d", sub),
            Route::Passive
        );

        // Not ours / not a shape we know.
        assert_eq!(route("lux/sync/user/other", sub), Route::Unknown);
        assert_eq!(
            route("lux/ctl/user/other/setup/s-1/frame", sub),
            Route::Unknown
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/verbs", sub),
            Route::Unknown
        );
        assert_eq!(route("lux/ctl/user/abc-123/setup/s-1", sub), Route::Unknown);
        assert_eq!(route("lux/ctl/user/abc-123", sub), Route::Unknown);
        assert_eq!(route("lux/1/buffer/set", sub), Route::Unknown);
    }

    #[test]
    fn gate_applies_only_known_versions_for_the_active_setup() {
        let overlay = Frame::buffer(vec![1, 2, 3]);
        assert_eq!(
            gate(overlay, "s-1", "s-1", "me00"),
            Some(RemoteApply::Overlay(vec![1, 2, 3]))
        );
        let channel = Frame::channel(10, 200);
        assert_eq!(
            gate(channel, "s-1", "s-1", "me00"),
            Some(RemoteApply::Channel { ch: 10, val: 200 })
        );

        // Inactive setup → dropped.
        assert_eq!(gate(Frame::channel(1, 1), "s-2", "s-1", "me00"), None);

        // Unknown version → dropped (parse it as the reader would).
        let future: Frame = serde_json::from_str(r#"{"v":9,"ch":1,"val":1}"#).expect("parses");
        assert_eq!(gate(future, "s-1", "s-1", "me00"), None);
    }

    #[test]
    fn gate_drops_our_own_frames_but_applies_other_sessions() {
        let own = Frame::channel(1, 255).with_src("me00");
        assert_eq!(gate(own, "s-1", "s-1", "me00"), None);

        let theirs = Frame::channel(1, 255).with_src("them");
        assert!(gate(theirs, "s-1", "s-1", "me00").is_some());

        // Unstamped (e.g. CLI-published) frames apply.
        assert!(gate(Frame::channel(1, 255), "s-1", "s-1", "me00").is_some());
    }
}
