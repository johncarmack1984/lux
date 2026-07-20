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

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rumqttc::{
    AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Packet, QoS, TlsConfiguration,
    Transport,
};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::watch;
use uuid::Uuid;

use lux_engine::auth::jwt_sub;
use lux_engine::ctl::{gate, guest_route, route, GuestRoute, RemoteApply, Route};
use lux_engine::tls::webpki_pem_bundle;

use crate::account::LuxAccount;
use crate::buffer::LuxBuffer;
use crate::lock::LockPolicy;
use crate::setup::LuxSetups;

/// Trailing-edge coalescing window for retained-config reconciles. Generous
/// because the triggers are human-paced (a patch edit, a grant change) and
/// arrive in bursts: a nudge, the pull it schedules, and the local commit that
/// pull produces are all one logical change.
const CONFIG_WINDOW: Duration = Duration::from_millis(750);

/// Trailing-edge coalescing window for the retained state echo (≤5 Hz) — a
/// remote surface needs truth, not every intermediate slider position.
const ECHO_WINDOW: Duration = Duration::from_millis(200);

/// Trailing-edge coalescing window for outgoing ctl frames (~25 Hz) — a fader
/// drag calls the input commands far faster than the wire needs; the outbox
/// keeps the latest value per touched slot and the flush publishes one batch.
const PUBLISH_WINDOW: Duration = Duration::from_millis(40);

/// How long after local input the incoming state echo is ignored. An applier's
/// echo lags a live drag by up to a few hundred milliseconds, so reflecting it
/// mid-drag would yank the slider backward (rubber-banding); local truth wins
/// while the user's hand is on the desk, and echoes re-converge within one
/// echo window once they stop.
const REFLECT_HOLDOFF: Duration = Duration::from_secs(2);

/// Consecutive connections that died after the sync subscribe was acked but
/// before the ctl one — the signature of a broker rejecting the ctl grant —
/// after which remote control latches off so sync stops flapping.
const CTL_FAILURE_LIMIT: u32 = 3;

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
    /// Presence cards from the user's *other* connections, keyed by session —
    /// the UI polls these through `list_remote_peers`.
    peers: Mutex<HashMap<String, lux_wire::ctl::PresenceCard>>,
    /// Outgoing ctl writes accumulated between flushes (see [`PUBLISH_WINDOW`]).
    outbox: Mutex<Outbox>,
    publish_pending: AtomicBool,
    /// When the user last drove this device locally — gates the state echo's
    /// reflection (see [`REFLECT_HOLDOFF`]).
    local_input_at: Mutex<Option<Instant>>,
    /// Consecutive ctl-suspect connection deaths (see [`CTL_FAILURE_LIMIT`]).
    ctl_failures: AtomicU32,
    /// A retained-config reconcile is queued (see [`refresh_shares`]).
    configs_pending: AtomicBool,
}

impl Default for LuxNudge {
    fn default() -> Self {
        Self {
            generation: watch::channel(0).0,
            presence: watch::channel(0).0,
            echo: Mutex::new(None),
            echo_pending: AtomicBool::new(false),
            configs_pending: AtomicBool::new(false),
            peers: Mutex::new(HashMap::new()),
            outbox: Mutex::new(Outbox::default()),
            publish_pending: AtomicBool::new(false),
            local_input_at: Mutex::new(None),
            ctl_failures: AtomicU32::new(0),
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

    fn upsert_peer(&self, session: &str, card: lux_wire::ctl::PresenceCard) {
        self.peers
            .lock_or_recover()
            .insert(session.to_owned(), card);
    }

    fn remove_peer(&self, session: &str) {
        self.peers.lock_or_recover().remove(session);
    }

    /// Forget every peer — the connection dropped, so the retained cards will
    /// be redelivered (and re-learned) on the next subscribe.
    fn clear_peers(&self) {
        self.peers.lock_or_recover().clear();
    }

    fn note_local_input(&self) {
        *self.local_input_at.lock_or_recover() = Some(Instant::now());
    }

    /// Whether local input happened within [`REFLECT_HOLDOFF`] — while true,
    /// incoming state echoes are ignored instead of reflected.
    fn within_reflect_holdoff(&self) -> bool {
        self.local_input_at
            .lock_or_recover()
            .is_some_and(|at| at.elapsed() < REFLECT_HOLDOFF)
    }

    /// The user's other live connections, stable-ordered for the UI poll.
    pub fn remote_peers(&self) -> Vec<RemotePeer> {
        // `session` comes from the topic, not the card body: the topic is what
        // the authorizer scoped, the body is whatever the publisher typed. They
        // agree for a peer's own cards, and a shared-control guest could
        // otherwise present itself under someone else's session id.
        let mut peers: Vec<RemotePeer> = self
            .peers
            .lock_or_recover()
            .iter()
            .map(|(session, card)| RemotePeer {
                session: session.clone(),
                setup_id: card.setup_id.clone(),
                name: card.name.clone(),
            })
            .collect();
        peers.sort_by(|a, b| a.session.cmp(&b.session));
        peers
    }
}

/// One of the user's other signed-in devices, as the UI sees it (a thinned
/// [`lux_wire::ctl::PresenceCard`]). A peer whose `setup_id` matches the
/// active setup is an applier for it: touches here apply there.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemotePeer {
    pub session: String,
    pub setup_id: String,
    pub name: String,
}

/// Outgoing ctl writes coalesced between flushes: latest value per touched
/// slot, plus at most one merged overlay. Drain order (overlay first, then
/// channels) matches local chronology — an overlay clears the pending channel
/// writes it covers, and later channel writes land after it at the applier.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outbox {
    overlay: Option<Vec<u8>>,
    channels: BTreeMap<u16, u8>,
}

impl Outbox {
    pub(crate) fn push_overlay(&mut self, bytes: Vec<u8>) {
        // The overlay supersedes any pending channel writes inside its range.
        self.channels.retain(|ch, _| usize::from(*ch) > bytes.len());
        match &mut self.overlay {
            // A shorter overlay after a longer one only rewrites its prefix.
            Some(pending) if pending.len() > bytes.len() => {
                pending[..bytes.len()].copy_from_slice(&bytes);
            }
            slot => *slot = Some(bytes),
        }
    }

    pub(crate) fn push_channel(&mut self, ch: u16, val: u8) {
        self.channels.insert(ch, val);
    }

    /// Everything pending as publishable frames, in apply order, leaving the
    /// outbox empty.
    pub(crate) fn drain(&mut self) -> Vec<lux_wire::ctl::Frame> {
        let mut frames = Vec::with_capacity(1 + self.channels.len());
        if let Some(bytes) = self.overlay.take() {
            frames.push(lux_wire::ctl::Frame::buffer(bytes));
        }
        for (ch, val) in std::mem::take(&mut self.channels) {
            frames.push(lux_wire::ctl::Frame::channel(ch, val));
        }
        frames
    }
}

/// The live connection's handle for publishing the retained state echo — and,
/// for a shared-control guest, control frames into an owner's space.
#[derive(Clone)]
pub(crate) struct EchoHandle {
    pub(crate) client: AsyncClient,
    pub(crate) sub: String,
    pub(crate) session: String,
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
    // A fresh sign-in gets a fresh chance at the ctl subscribe (see
    // CTL_FAILURE_LIMIT).
    state.ctl_failures.store(0, Ordering::SeqCst);

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
/// The live connection, if there is one — the guest publisher's handle into
/// the same socket everything else on this channel uses.
pub(crate) fn connection<R: Runtime>(app: &AppHandle<R>) -> Option<EchoHandle> {
    app.state::<LuxNudge>().echo.lock_or_recover().clone()
}

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

/// The setup ids a shared-control guest may currently render, from the
/// caller's own grants (`GET /shares`). Pure so the reconcile below stays
/// testable without a broker: given what is granted and what exists locally,
/// exactly these setups should carry a retained config and every other local
/// setup should carry none.
fn shared_setup_ids(
    granted: &[lux_wire::shares::Grant],
    local: &[uuid::Uuid],
) -> std::collections::HashSet<uuid::Uuid> {
    let local: std::collections::HashSet<uuid::Uuid> = local.iter().copied().collect();
    granted
        .iter()
        .filter_map(|g| uuid::Uuid::parse_str(&g.setup_id).ok())
        .filter(|id| local.contains(id))
        .collect()
}

/// Publish the retained compiled setup for every shared setup, and clear it for
/// every other setup on the account.
///
/// Deliberately a full reconcile rather than incremental publish/clear calls,
/// and it keeps no record of what it published last. A guest's whole view of a
/// setup is this retained frame, so the failure that matters is a stale one
/// outliving its grant — and any bookkeeping of "what did I publish" is exactly
/// the thing that drifts when the app is closed while a grant is revoked from
/// another device. Deriving the answer from current state every time cannot
/// drift, and clearing a topic that holds nothing is a no-op.
///
/// The one case this cannot cover is a setup deleted locally: it is gone from
/// the account, so nothing here names it. [`clear_config`] handles that at the
/// deletion site, before the setup disappears.
pub fn reconcile_configs<R: Runtime>(app: &AppHandle<R>, granted: &[lux_wire::shares::Grant]) {
    let Some(echo) = app.state::<LuxNudge>().echo.lock_or_recover().clone() else {
        return; // no connection; the next connect reconciles from scratch
    };
    for (setup_id, config) in config_plan(&app.state::<LuxSetups>().all(), granted) {
        // `None` is a clear: an empty retained payload deletes the retained
        // message, the same idiom the presence card uses.
        let payload = match config.as_ref().map(serde_json::to_vec).transpose() {
            Ok(payload) => payload.unwrap_or_default(),
            Err(e) => {
                log::warn!("could not compile setup {setup_id} for sharing: {e}");
                continue;
            }
        };
        let topic = lux_wire::ctl::config_topic(&echo.sub, &setup_id.to_string());
        let client = echo.client.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client.publish(&topic, QoS::AtMostOnce, true, payload).await {
                log::debug!("config publish to {topic} failed (connection likely down): {e}");
            }
        });
    }
}

/// The retained-config writes one reconcile implies: a compiled payload for
/// every shared setup, `None` (clear) for every other setup on the account.
///
/// Split out from the publish loop because *this* is the part worth being sure
/// about — which setups a guest can see — and it is a pure function of local
/// setups plus current grants, so it can be checked without a broker.
fn config_plan(
    setups: &[crate::setup::Setup],
    granted: &[lux_wire::shares::Grant],
) -> Vec<(uuid::Uuid, Option<lux_wire::ctl::Config>)> {
    let shared = shared_setup_ids(granted, &setups.iter().map(|s| s.id).collect::<Vec<_>>());
    setups
        .iter()
        .map(|setup| {
            let config = shared.contains(&setup.id).then(|| setup.compile());
            (setup.id, config)
        })
        .collect()
}

/// Subscribe to the topics this device's received grants allow it to read.
///
/// Additive and idempotent: re-subscribing to a topic already held is a no-op
/// at the broker, and a grant that ended simply stops being subscribed on the
/// next connection. There is deliberately no unsubscribe — the authorizer stops
/// delivering when it drops the grant from the policy, and
/// [`crate::guest::adopt_grants`] has already forgotten the content, so an
/// extra subscription buys an attacker nothing and costs a round trip.
fn subscribe_shared<R: Runtime>(app: &AppHandle<R>, received: &[lux_wire::shares::ReceivedGrant]) {
    let Some(echo) = app.state::<LuxNudge>().echo.lock_or_recover().clone() else {
        return;
    };
    for topic in crate::guest::subscriptions(received) {
        let client = echo.client.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client.subscribe(&topic, QoS::AtMostOnce).await {
                log::debug!("could not subscribe to {topic}: {e}");
            }
        });
    }
}

/// Clear one setup's retained config. Called when a setup is deleted, while its
/// id is still known — after that, [`reconcile_configs`] can no longer name it.
pub fn clear_config<R: Runtime>(app: &AppHandle<R>, setup_id: uuid::Uuid) {
    let Some(echo) = app.state::<LuxNudge>().echo.lock_or_recover().clone() else {
        return;
    };
    let topic = lux_wire::ctl::config_topic(&echo.sub, &setup_id.to_string());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = echo
            .client
            .publish(&topic, QoS::AtMostOnce, true, Vec::<u8>::new())
            .await
        {
            log::debug!("config clear on {topic} failed (connection likely down): {e}");
        }
    });
}

/// Fetch the caller's shares and reconcile both halves of the feature against
/// them: the retained configs this device publishes as an owner, and the
/// grants it holds as a guest. One pull answers both, so they cannot disagree
/// about what is shared.
///
/// The entry point for anything that changes *who* can see a setup — a shares
/// nudge, and the connect handshake, where a grant may have changed while this
/// device was offline.
/// Coalesced, so every caller can be naive: a cloud pull, the nudge that caused
/// it, and the local commit it produces all land in one reconcile instead of
/// three round trips.
pub fn refresh_shares(app: &AppHandle) {
    let state = app.state::<LuxNudge>();
    if state.echo.lock_or_recover().is_none() {
        return;
    }
    if state.configs_pending.swap(true, Ordering::SeqCst) {
        return; // one is already queued and will see this change too
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CONFIG_WINDOW).await;
        app.state::<LuxNudge>()
            .configs_pending
            .store(false, Ordering::SeqCst);
        match crate::cloud::shares(&app).await {
            Ok(shares) => {
                reconcile_configs(&app, &shares.granted);
                crate::guest::adopt_grants(&app, &shares.received);
                subscribe_shared(&app, &shares.received);
            }
            // Leave the retained configs exactly as they are. Publishing on a
            // guess is worse than publishing late: the pull retries, and a
            // grant that was revoked is already unreadable by the guest whose
            // policy no longer covers it.
            Err(e) => log::warn!("could not refresh shares for config publish: {e}"),
        }
    });
}

/// Queue a locally-entered overlay write (the color-picker path) for remote
/// publish. Called from the command layer only — never from an apply path —
/// which is the structural loop guard: remotely-applied frames re-enter the
/// buffer, not the outbox. Fast no-op when the user channel is down.
pub fn publish_input_overlay<R: Runtime>(app: &AppHandle<R>, bytes: Vec<u8>) {
    let state = app.state::<LuxNudge>();
    if state.echo.lock_or_recover().is_none() {
        return;
    }
    state.note_local_input();
    state.outbox.lock_or_recover().push_overlay(bytes);
    schedule_publish(app);
}

/// Queue a locally-entered single-slot write (the fader path) for remote
/// publish. Same command-layer-only contract as [`publish_input_overlay`].
pub fn publish_input_channel<R: Runtime>(app: &AppHandle<R>, ch: u16, val: u8) {
    let state = app.state::<LuxNudge>();
    if state.echo.lock_or_recover().is_none() {
        return;
    }
    state.note_local_input();
    state.outbox.lock_or_recover().push_channel(ch, val);
    schedule_publish(app);
}

/// Trailing-edge flush of the outbox onto the wire: frames are addressed to
/// the setup active at flush time, stamped with the connection's session id
/// (so our own subscription echo is dropped by the gate), and published in
/// apply order on the one connection, which preserves ordering end to end.
fn schedule_publish<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LuxNudge>();
    if state.publish_pending.swap(true, Ordering::SeqCst) {
        return; // a flush is already queued and will pick these writes up
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PUBLISH_WINDOW).await;
        let state = app.state::<LuxNudge>();
        state.publish_pending.store(false, Ordering::SeqCst);
        let Some(echo) = state.echo.lock_or_recover().clone() else {
            // Connection died inside the window; drop the batch — the retained
            // state echo re-synchronizes everyone once a connection returns.
            state.outbox.lock_or_recover().drain();
            return;
        };
        let frames = state.outbox.lock_or_recover().drain();
        let setup_id = app.state::<LuxSetups>().active_id().to_string();
        let topic = lux_wire::ctl::frame_topic(&echo.sub, &setup_id);
        for frame in frames {
            let Ok(payload) = serde_json::to_vec(&frame.with_src(&echo.session)) else {
                continue;
            };
            if let Err(e) = echo
                .client
                .publish(topic.clone(), QoS::AtMostOnce, false, payload)
                .await
            {
                log::debug!("ctl publish failed (connection likely down): {e}");
                return;
            }
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

    let state = app.state::<LuxNudge>();

    // Two subscribe packets, sync first, so the nudge subscription never
    // shares fate with the ctl one: if the broker ever rejects the ctl grant
    // (which kills the whole connection), sync reconnects on its own, and
    // after CTL_FAILURE_LIMIT such deaths remote control latches off for this
    // sign-in while sync carries on unharmed.
    let (client, mut eventloop) = AsyncClient::new(opts, 10);
    if let Err(e) = client
        .subscribe(lux_wire::nudge::user_topic(sub), QoS::AtMostOnce)
        .await
    {
        log::warn!("nudge: could not queue subscribe: {e}");
        return false;
    }
    let try_ctl = state.ctl_failures.load(Ordering::SeqCst) < CTL_FAILURE_LIMIT;
    if try_ctl {
        if let Err(e) = client
            .subscribe(lux_wire::ctl::user_filter(sub), QoS::AtMostOnce)
            .await
        {
            log::warn!("could not queue the remote-control subscribe: {e}");
        }
    }

    let mut presence_rx = state.presence.subscribe();
    presence_rx.borrow_and_update();
    // SubAcks arrive in subscribe order: 1st = sync live, 2nd = ctl live. All
    // ctl publishing (presence, echoes) waits for the 2nd, so a connection
    // without the ctl grant never attempts a publish the policy would refuse.
    let mut acks = 0u32;

    loop {
        tokio::select! {
            _ = generation.changed() => {
                if acks >= 2 {
                    // Graceful goodbye: clear the retained presence card so
                    // other surfaces grey out immediately (the Last Will only
                    // fires on ungraceful drops).
                    let _ = client
                        .publish(presence_topic.clone(), QoS::AtMostOnce, true, Vec::<u8>::new())
                        .await;
                }
                let _ = client.disconnect().await;
                state.clear_echo(&session);
                state.clear_peers();
                return false; // superseded — the outer loop exits
            }
            _ = presence_rx.changed() => {
                if acks >= 2 {
                    publish_presence(&client, app, sub, &session).await;
                }
            }
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    acks += 1;
                    if acks == 1 {
                        log::info!("user channel connected; change nudges live");
                        *backoff_secs = 1;
                        // On-(re)connect pull: cover anything nudged while offline.
                        crate::cloud::schedule_sync(app);
                    }
                    if acks == 2 {
                        log::info!("remote control live on the user channel");
                        state.ctl_failures.store(0, Ordering::SeqCst);
                        state.set_echo(EchoHandle {
                            client: client.clone(),
                            sub: sub.to_owned(),
                            session: session.clone(),
                        });
                        publish_presence(&client, app, sub, &session).await;
                        // Refresh the retained truth for whoever is watching.
                        schedule_state_echo(app);
                        // …including shared-control guests, whose entire view
                        // of a setup is its retained config. Grants can change
                        // while this device is offline, so reconcile on every
                        // connect rather than trusting what a past session
                        // published.
                        refresh_shares(app);
                    }
                }
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    match route(&publish.topic, sub) {
                        Route::Nudge => {
                            // Opaque frame — never parsed; any frame means "pull
                            // now", and since shared control the pull covers who
                            // can see a setup as well as what is in it.
                            log::debug!("nudge received; scheduling sync");
                            crate::cloud::schedule_sync(app);
                            refresh_shares(app);
                        }
                        Route::Frame { setup_id } => {
                            apply_frame(app, &publish.payload, setup_id, &session);
                        }
                        Route::State { setup_id } => {
                            reflect_state(app, &publish.payload, setup_id, &session);
                        }
                        Route::Presence { session: card_session } => {
                            update_presence(app, &publish.payload, card_session, &session);
                        }
                        Route::Config => {
                            // Our own compiled setup, echoed back by the `#`
                            // subscribe. We published it; nothing to learn.
                        }
                        // Not our own space — the other place a publish can
                        // legitimately come from is an owner who shared a setup
                        // with us.
                        Route::Unknown => match guest_route(&publish.topic, sub) {
                            Some(GuestRoute::Config { owner_sub, setup_id }) => {
                                crate::guest::receive_config(
                                    app,
                                    owner_sub,
                                    setup_id,
                                    &publish.payload,
                                );
                            }
                            Some(GuestRoute::State { owner_sub, setup_id }) => {
                                crate::guest::receive_state(
                                    app,
                                    owner_sub,
                                    setup_id,
                                    &publish.payload,
                                );
                            }
                            None => log::debug!(
                                "ignoring publish on unexpected topic {}",
                                publish.topic
                            ),
                        },
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    log::info!("nudge connection error (will reconnect): {e}");
                    if try_ctl && acks == 1 {
                        // Sync was acked but the connection died before the
                        // ctl ack — the signature of a rejected ctl subscribe.
                        let failures = state.ctl_failures.fetch_add(1, Ordering::SeqCst) + 1;
                        if failures == CTL_FAILURE_LIMIT {
                            log::warn!(
                                "connection died right after the remote-control subscribe \
                                 {failures} times; disabling remote control until next sign-in"
                            );
                        }
                    }
                    state.clear_echo(&session);
                    state.clear_peers();
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
pub(crate) fn device_name() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// Reflect an applier's state echo: overwrite the live buffer and update the
/// UI/persistence **without rendering, publishing, or re-echoing** — remote
/// state must never re-enter the output or publish paths, which is what makes
/// two devices echoing at each other impossible by construction.
fn reflect_state(app: &AppHandle, payload: &[u8], frame_setup: &str, own_session: &str) {
    let frame: lux_wire::ctl::Frame = match serde_json::from_slice(payload) {
        Ok(frame) => frame,
        Err(e) => {
            log::warn!("ignoring unreadable state echo: {e}");
            return;
        }
    };
    if frame.version() != lux_wire::ctl::VERSION {
        log::debug!(
            "dropping state echo with unknown version {}",
            frame.version()
        );
        return;
    }
    if frame.src() == Some(own_session) {
        return; // our own echo delivered back
    }
    if frame_setup != app.state::<LuxSetups>().active_id().to_string() {
        return;
    }
    if app.state::<LuxNudge>().within_reflect_holdoff() {
        // The user is driving this device right now; an echo lags their hand
        // by up to a few hundred ms and would rubber-band the faders. Local
        // truth wins until they let go, then echoes re-converge.
        log::trace!("ignoring state echo during local input");
        return;
    }
    let lux_wire::ctl::Frame::Buffer { buffer, .. } = frame else {
        log::debug!("state echo carried a non-buffer frame; ignoring");
        return;
    };
    crate::buffer::reflect_remote_state(app, &buffer);
}

/// Track a peer's retained presence card (empty payload = the peer is gone).
/// Our own card comes back through the wildcard subscription too — skipped, so
/// the peers list is always "the user's *other* devices".
fn update_presence(app: &AppHandle, payload: &[u8], card_session: &str, own_session: &str) {
    if card_session == own_session {
        return;
    }
    let state = app.state::<LuxNudge>();
    if payload.is_empty() {
        state.remove_peer(card_session);
        return;
    }
    let card: lux_wire::ctl::PresenceCard = match serde_json::from_slice(payload) {
        Ok(card) => card,
        Err(e) => {
            log::warn!("ignoring unreadable presence card: {e}");
            return;
        }
    };
    if card.v != lux_wire::ctl::VERSION {
        log::debug!("dropping presence card with unknown version {}", card.v);
        return;
    }
    state.upsert_peer(card_session, card);
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

#[cfg(test)]
mod tests {
    use super::*;
    use lux_wire::ctl::Frame;

    #[test]
    fn shared_setups_are_the_granted_ones_that_still_exist_here() {
        use std::collections::HashSet;
        let a = uuid::Uuid::from_u128(1);
        let b = uuid::Uuid::from_u128(2);
        let gone = uuid::Uuid::from_u128(3);
        let grant = |id: uuid::Uuid| lux_wire::shares::Grant {
            contact_sub: "c".into(),
            contact_label: "c@example.com".into(),
            setup_id: id.to_string(),
            setup_name: None,
            label: None,
            created_at: 0,
        };

        // Two contacts on one setup is one setup, not two configs.
        assert_eq!(
            shared_setup_ids(&[grant(a), grant(a), grant(b)], &[a, b]),
            HashSet::from([a, b])
        );

        // A grant naming a setup this device doesn't have — deleted elsewhere,
        // or not pulled yet — publishes nothing. There is no setup to compile,
        // and inventing an empty one would blank a guest's desk.
        assert!(shared_setup_ids(&[grant(gone)], &[a]).is_empty());

        // No grants: every local setup falls into the reconcile's clear set.
        assert!(shared_setup_ids(&[], &[a, b]).is_empty());

        // A malformed setup id is skipped, not panicked on.
        let mut bad = grant(a);
        bad.setup_id = "not-a-uuid".into();
        assert!(shared_setup_ids(&[bad], &[a]).is_empty());
    }

    #[test]
    fn the_plan_publishes_only_shared_setups_and_clears_every_other() {
        let setup = |id: u128, name: &str| crate::setup::Setup {
            id: uuid::Uuid::from_u128(id),
            name: name.to_owned(),
            universe: 1,
            fixtures: Vec::new(),
            updated_at: None,
            dirty: false,
        };
        let grant = |id: u128| lux_wire::shares::Grant {
            contact_sub: "c".into(),
            contact_label: "c@example.com".into(),
            setup_id: uuid::Uuid::from_u128(id).to_string(),
            setup_name: None,
            label: None,
            created_at: 0,
        };
        let setups = vec![setup(1, "Shared"), setup(2, "Private")];

        let plan = config_plan(&setups, &[grant(1)]);
        assert_eq!(plan.len(), 2, "every local setup gets a decision");
        let shared = plan.iter().find(|(id, _)| *id == setups[0].id).unwrap();
        let private = plan.iter().find(|(id, _)| *id == setups[1].id).unwrap();
        assert_eq!(
            shared.1.as_ref().map(|c| c.name.as_str()),
            Some("Shared"),
            "a granted setup publishes its compiled config"
        );
        assert!(
            private.1.is_none(),
            "an ungranted setup is cleared, not skipped — a config from a \
             revoked grant must not outlive it"
        );

        // Revoking the last grant turns the publish into a clear on the very
        // next reconcile, with no memory of what was published before.
        let after_revoke = config_plan(&setups, &[]);
        assert!(after_revoke.iter().all(|(_, config)| config.is_none()));
    }

    #[test]
    fn outbox_coalesces_channels_to_latest_value() {
        let mut outbox = Outbox::default();
        outbox.push_channel(10, 1);
        outbox.push_channel(10, 2);
        outbox.push_channel(3, 9);
        assert_eq!(
            outbox.drain(),
            vec![Frame::channel(3, 9), Frame::channel(10, 2)]
        );
        assert_eq!(outbox.drain(), vec![]); // drained empty
    }

    #[test]
    fn outbox_overlay_supersedes_covered_channels_and_drains_first() {
        let mut outbox = Outbox::default();
        outbox.push_channel(2, 7); // inside the overlay range — superseded
        outbox.push_channel(100, 42); // outside — survives
        outbox.push_overlay(vec![1, 2, 3, 4, 5, 6]);
        outbox.push_channel(2, 8); // after the overlay — applies after it
        assert_eq!(
            outbox.drain(),
            vec![
                Frame::buffer(vec![1, 2, 3, 4, 5, 6]),
                Frame::channel(2, 8),
                Frame::channel(100, 42),
            ]
        );
    }

    #[test]
    fn outbox_merges_a_shorter_overlay_onto_a_longer_pending_one() {
        let mut outbox = Outbox::default();
        outbox.push_overlay(vec![9, 9, 9, 9]);
        outbox.push_overlay(vec![1, 2]);
        assert_eq!(outbox.drain(), vec![Frame::buffer(vec![1, 2, 9, 9])]);

        // A longer (or equal) overlay simply replaces the pending one.
        let mut outbox = Outbox::default();
        outbox.push_overlay(vec![1, 2]);
        outbox.push_overlay(vec![5, 5, 5]);
        assert_eq!(outbox.drain(), vec![Frame::buffer(vec![5, 5, 5])]);
    }

    #[test]
    fn reflect_holdoff_gates_recent_local_input() {
        let state = LuxNudge::default();
        assert!(!state.within_reflect_holdoff()); // no input yet — echoes reflect

        state.note_local_input();
        assert!(state.within_reflect_holdoff()); // hand on the desk — hold off

        *state.local_input_at.lock_or_recover() = Some(Instant::now() - REFLECT_HOLDOFF * 2);
        assert!(!state.within_reflect_holdoff()); // input long past — reflect again
    }
}
