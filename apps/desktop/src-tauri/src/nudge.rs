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
//!   changes made locally at this device. AWS IoT Core stores one retained
//!   publish per topic per second and drops the rest, so echoes go out live at
//!   that rate and this device retains at most one per
//!   [`lux_engine::ctl::RETAIN_INTERVAL`], plus a trailing one so the stored
//!   copy catches up to the final state.
//!
//! On connect the device takes in the active setup's retained echo instead of
//! publishing its own. The buffer it holds then is only what it last knew:
//! restored from disk, or frozen while the app was suspended. A node seeds
//! from every echo it hears, so publishing that buffer would put a stale look
//! back on the rig. AWS IoT Core delivers a retained message only to a
//! subscription that names its topic, never through a wildcard. So that
//! `state` topic is subscribed by name next to the ctl wildcard, and again on
//! every setup switch. Until the device holds the setup's state it publishes
//! no echo; its input still goes out as frames. Edits it made that the broker
//! never confirmed, offline or in flight when a connection dropped, are kept
//! slot by slot ([`Pending`]). When the state comes in they go back on top of
//! it and out as frames, so what this device changed reaches the rig and what
//! it merely missed doesn't get put back.
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

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use rumqttc::{
    AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Packet, QoS, TlsConfiguration,
    Transport,
};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::watch;
use uuid::Uuid;

use lux_engine::auth::jwt_sub;
use lux_engine::ctl::{gate, guest_route, retain_wait, route, GuestRoute, RemoteApply, Route};
use lux_engine::tls::webpki_pem_bundle;

use crate::account::LuxAccount;
use crate::buffer::{LuxBuffer, UNIVERSE_SIZE};
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

/// How long after the broker acks a setup's `state` subscription to wait for
/// its retained echo before concluding there is none — a setup nothing has
/// echoed yet. When the echo exists it follows the ack directly.
const LEARN_GRACE: Duration = Duration::from_millis(1000);

/// How long local edits the broker never confirmed stay worth laying over a
/// state someone else stored meanwhile (see [`Pending`]). Past this the device
/// has likely been asleep or idle, and the other change is likely the newer.
/// When the stored state is the one this device had just before going quiet,
/// nothing expires: the buffer is kept whole (see [`Seen`]).
const PENDING_LIFETIME: Duration = Duration::from_secs(60);

fn nudge_endpoint() -> Option<String> {
    Some(crate::endpoints::effective().nudge_endpoint.clone()).filter(|e| !e.is_empty())
}

/// Tauri-managed listener lifecycle. The `generation` watch value is a
/// counter: [`start`] bumps it and spawns a listener bound to the new
/// generation, so a stale listener (previous sign-in, or a sign-out via
/// [`stop`]) sees the bump and exits — at most one listener is ever live.
/// `presence` is bumped by [`active_setup_changed`] so the live connection
/// republishes its card and subscribes the new setup's state echo without
/// reconnecting; `echo` is the live connection's publish handle for
/// [`schedule_state_echo`].
pub struct LuxNudge {
    generation: watch::Sender<u64>,
    presence: watch::Sender<u64>,
    echo: Mutex<Option<EchoHandle>>,
    /// Stamped as `src` on every frame and echo this process publishes. Per
    /// process, not per connection like the client-id session, so the
    /// device's own traffic reads as its own across reconnects.
    publisher: String,
    /// The state echoes this process has heard from the broker for the active
    /// setup since it was switched to, by content (see [`Seen`]).
    seen: Mutex<Seen>,
    /// The setup the queued state echo belongs to, with the switch count when
    /// it was queued (see [`schedule_state_echo`]).
    echo_for: Mutex<Option<(String, u64)>>,
    /// Bumped by every setup switch. Queued echo work carries the count it was
    /// queued under and is dropped if it changed, so nothing queued before a
    /// switch goes out after it, even on a quick switch back to the same setup.
    switches: AtomicU64,
    /// When this device last retained a state echo, per setup: retained
    /// publishes are spaced by [`lux_engine::ctl::RETAIN_INTERVAL`].
    last_retained: Mutex<HashMap<String, Instant>>,
    /// The (setup id, switch count) a trailing retained echo is queued for:
    /// live echoes have moved past the stored copy.
    retain_queued: Mutex<Option<(String, u64)>>,
    /// Local edits the broker hasn't confirmed yet (see [`Pending`]).
    pending: Mutex<Pending>,
    /// Serializes a setup switch (stopping the fade, forgetting what was held,
    /// blanking the buffer) against the remote path's reads and writes of the
    /// buffer: reflecting an echo, snapshotting one, a fade tick. Without it,
    /// one of those can land a moment after the blank, on the new setup.
    desk: Mutex<()>,
    /// Presence cards from the user's *other* connections, keyed by session —
    /// the UI polls these through `list_remote_peers`.
    peers: Mutex<HashMap<String, lux_wire::ctl::PresenceCard>>,
    /// Outgoing ctl writes accumulated between flushes (see [`PUBLISH_WINDOW`]).
    outbox: Mutex<Outbox>,
    publish_pending: AtomicBool,
    /// When the user last drove this device locally — gates the state echo's
    /// reflection (see [`REFLECT_HOLDOFF`]).
    local_input_at: Mutex<Option<Instant>>,
    /// Which setup's state the live connection holds, and how firmly (see
    /// [`Held`]). Until it matches the live connection and the active setup,
    /// the buffer is only what this device last knew. Keyed by session, so a
    /// new connection starts empty; a setup switch clears it.
    held: Mutex<Option<Held>>,
    /// Bumped by every grace timer armed and every setup switch, so only the
    /// latest timer may conclude there is no retained echo (see
    /// [`arm_learn_grace`]).
    learn_epoch: AtomicU64,
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
            publisher: Uuid::new_v4().simple().to_string()[..8].to_owned(),
            seen: Mutex::new(Seen::default()),
            echo_for: Mutex::new(None),
            switches: AtomicU64::new(0),
            last_retained: Mutex::new(HashMap::new()),
            retain_queued: Mutex::new(None),
            pending: Mutex::new(Pending::default()),
            desk: Mutex::new(()),
            configs_pending: AtomicBool::new(false),
            peers: Mutex::new(HashMap::new()),
            outbox: Mutex::new(Outbox::default()),
            publish_pending: AtomicBool::new(false),
            local_input_at: Mutex::new(None),
            held: Mutex::new(None),
            learn_epoch: AtomicU64::new(0),
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

    /// How firmly connection `session` holds `setup_id`'s state, if at all.
    fn held(&self, session: &str, setup_id: &str) -> Option<Grip> {
        self.held
            .lock_or_recover()
            .as_ref()
            .filter(|h| h.session == session && h.setup_id == setup_id)
            .map(|h| h.grip)
    }

    fn hold(&self, session: &str, setup_id: &str, grip: Grip) {
        *self.held.lock_or_recover() = Some(Held {
            session: session.to_owned(),
            setup_id: setup_id.to_owned(),
            grip,
        });
    }

    /// Claim the retained slot for `setup_id` if the per-topic quota allows
    /// one now; returns whether this echo is the retained one.
    fn take_retain_slot(&self, setup_id: &str) -> bool {
        let mut last = self.last_retained.lock_or_recover();
        let now = Instant::now();
        if retain_wait(last.get(setup_id).copied(), now).is_zero() {
            last.insert(setup_id.to_owned(), now);
            true
        } else {
            false
        }
    }

    /// How long until `setup_id` may have another retained echo.
    fn retain_wait(&self, setup_id: &str) -> Duration {
        let last = self.last_retained.lock_or_recover();
        retain_wait(last.get(setup_id).copied(), Instant::now())
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

/// The setup whose state a connection holds. Until it holds one, the buffer is
/// only what this device last knew, so it publishes no echo for that setup.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    session: String,
    setup_id: String,
    grip: Grip,
}

/// How firmly a connection holds a setup's state. Anything short of [`Firm`]
/// is the grace timer's guess that nothing is stored, so any echo that turns
/// up, retained or live, is still taken in (see [`landing`]).
///
/// [`Firm`]: Grip::Firm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grip {
    /// One grace period passed with no stored echo. The topic was asked for
    /// again and pending edits went out as frames, but no full-buffer echo
    /// goes out on a guess.
    Asking,
    /// A second grace period passed too: nothing is stored for the setup. The
    /// buffer stands, and its first echo makes the hold firm.
    Assumed,
    /// Took in a real echo, or published one.
    Firm,
}

/// Local edits the broker hasn't confirmed: slots this device changed itself
/// (input, a scene fade, the Discord rail) and their latest values. A peer's
/// frame applied here is the peer's to deliver, so it never lands here.
///
/// An echo carrying a slot's value confirms it, whoever sent it. What is left
/// when a connection takes in the setup's state goes back on top of that state
/// and out as frames: the whole buffer never travels on the strength of a
/// local edit, so a device that missed changes while away can't put them
/// back.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Pending {
    slots: BTreeMap<u16, u8>,
    /// When the latest edit was made: wall-clock time, so a device that slept
    /// counts the time asleep.
    at: Option<SystemTime>,
}

impl Pending {
    /// Note edits, keeping only real slots: one past the universe would never
    /// be confirmed by an echo.
    pub(crate) fn record(&mut self, edits: impl IntoIterator<Item = (u16, u8)>, now: SystemTime) {
        let mut any = false;
        for (slot, val) in edits {
            if (1..=UNIVERSE_SIZE).contains(&usize::from(slot)) {
                self.slots.insert(slot, val);
                any = true;
            }
        }
        if any {
            self.at = Some(now);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Drop the slots `echo` already carries (slot N is `echo[N - 1]`).
    pub(crate) fn confirm(&mut self, echo: &[u8]) {
        self.slots.retain(|&slot, &mut val| {
            usize::from(slot)
                .checked_sub(1)
                .and_then(|index| echo.get(index))
                != Some(&val)
        });
        if self.slots.is_empty() {
            self.at = None;
        }
    }

    /// Drop `slots`: a peer's frame set them after us, and the latest touch
    /// wins.
    pub(crate) fn supersede(&mut self, slots: impl IntoIterator<Item = u16>) {
        for slot in slots {
            self.slots.remove(&slot);
        }
        if self.slots.is_empty() {
            self.at = None;
        }
    }

    /// Every edit, however old: over a buffer that is kept whole, they are all
    /// still this device's to deliver.
    pub(crate) fn all(&self) -> Vec<(u16, u8)> {
        self.slots.iter().map(|(&slot, &val)| (slot, val)).collect()
    }

    /// The edits still worth delivering at `now`, dropping them all first if
    /// the latest is older than [`PENDING_LIFETIME`].
    pub(crate) fn live(&mut self, now: SystemTime) -> Vec<(u16, u8)> {
        let expired = self
            .at
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|age| age > PENDING_LIFETIME);
        if expired {
            self.clear();
        }
        self.slots.iter().map(|(&slot, &val)| (slot, val)).collect()
    }

    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.at = None;
    }
}

/// The state echoes this process has heard from the broker for the active
/// setup since it was switched to — its own included, as they come back —
/// kept by content hash with when each was last noted, newest last.
///
/// It answers one question when a connection takes in a stored echo after a
/// gap: is this a state the device had in its last moments before going
/// quiet? If so, and the device made edits since, nothing moved on the
/// broker's side while it was away, and the buffer is kept whole — an
/// operator who carried on through a dropped connection doesn't see the rig
/// snap back when it returns. Content, not `src`, is the test, because several
/// appliers echo the same change and whichever retained publish lands first
/// is the one the broker keeps. Only the last [`Seen::RECENT`] before the
/// newest note counts: the stored copy trails the latest echo by no more than
/// that under the retained quota. An older state the rig has since been
/// returned to — a scene recalled again — is somebody's new change. Only
/// what comes back is noted, never what is sent: a connection that died
/// unnoticed swallows sends for up to a keepalive, and noting them would
/// move "the last moments" past the state the broker actually kept.
#[derive(Debug, Default)]
pub(crate) struct Seen {
    entries: VecDeque<(u64, Instant)>,
}

impl Seen {
    const WINDOW: usize = 64;
    const RECENT: Duration = Duration::from_millis(3000);

    pub(crate) fn note(&mut self, buffer: &[u8]) {
        self.note_at(buffer, Instant::now());
    }

    fn note_at(&mut self, buffer: &[u8], at: Instant) {
        let hash = content_hash(buffer);
        match self.entries.back_mut() {
            Some((newest, when)) if *newest == hash => *when = at,
            _ => {
                if self.entries.len() == Self::WINDOW {
                    self.entries.pop_front();
                }
                self.entries.push_back((hash, at));
            }
        }
    }

    /// Whether `buffer` was noted within [`Seen::RECENT`] of the newest note.
    pub(crate) fn recent(&self, buffer: &[u8]) -> bool {
        let Some(&(_, newest)) = self.entries.back() else {
            return false;
        };
        let hash = content_hash(buffer);
        self.entries
            .iter()
            .any(|&(h, at)| h == hash && newest.saturating_duration_since(at) <= Self::RECENT)
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

fn content_hash(buffer: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(buffer);
    hasher.finish()
}

/// The edits an overlay makes: slot N takes `bytes[N - 1]`.
fn overlay_edits(bytes: &[u8]) -> impl Iterator<Item = (u16, u8)> + '_ {
    bytes
        .iter()
        .enumerate()
        .filter_map(|(index, &val)| u16::try_from(index + 1).ok().map(|slot| (slot, val)))
}

/// What a state echo for the active setup does to the buffer (see
/// [`landing`]).
#[derive(Debug, PartialEq, Eq)]
enum Landing {
    /// Take it in as the setup's state, with pending edits back on top.
    Learn,
    /// Overwrite the buffer with it: the latest word on a setup already held.
    Live,
    /// Leave the buffer alone.
    Skip,
}

/// - Until this connection holds the setup's state firmly, the buffer is only
///   what this device last knew: restored from disk, frozen while it was
///   offline, or blanked by a setup switch, or a grace timer's guess that
///   nothing is stored. The first real echo, retained or live, is learned
///   whatever else is happening, even mid-drag. The drag's own edits are
///   pending and go back on top.
/// - After that, a retained echo is the broker's stored copy from when we
///   subscribed. It can be older than what we hold — the broker doesn't order
///   it against live traffic — so it is skipped.
/// - A live echo overwrites unless the user is driving this device right now:
///   it lags their hand by up to a few hundred ms and would rubber-band the
///   faders ([`REFLECT_HOLDOFF`]).
fn landing(retained: bool, grip: Option<Grip>, within_holdoff: bool) -> Landing {
    match grip {
        None | Some(Grip::Asking | Grip::Assumed) => Landing::Learn,
        Some(Grip::Firm) if retained || within_holdoff => Landing::Skip,
        Some(Grip::Firm) => Landing::Live,
    }
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

/// The live connection, if there is one — the guest publisher's handle into
/// the same socket everything else on this channel uses.
pub(crate) fn connection<R: Runtime>(app: &AppHandle<R>) -> Option<EchoHandle> {
    app.state::<LuxNudge>().echo.lock_or_recover().clone()
}

/// Hold the desk (see [`LuxNudge`]'s `desk`): a setup switch takes it around
/// stopping the fade and blanking, a fade tick around each write.
pub(crate) fn desk<R: Runtime>(app: &AppHandle<R>) -> MutexGuard<'_, ()> {
    app.state::<LuxNudge>().inner().desk.lock_or_recover()
}

/// The active setup is switching (`cmd::activate`, under the desk, before the
/// buffer is blanked for the new one). Nothing this device knew of the old
/// setup's state describes its buffer any more. It holds nothing and owes
/// nothing, and nothing queued for the old setup goes out after the switch:
/// no echo, no frame, no grace timer's conclusion.
pub fn forget_setup_state<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LuxNudge>();
    state.switches.fetch_add(1, Ordering::SeqCst);
    state.learn_epoch.fetch_add(1, Ordering::SeqCst);
    state.seen.lock_or_recover().clear();
    *state.held.lock_or_recover() = None;
    state.pending.lock_or_recover().clear();
    state.outbox.lock_or_recover().drain();
}

/// Start the input holdoff (see [`REFLECT_HOLDOFF`]) for a change made here
/// that no fader drag announces: a Discord command, a scene fade. A live echo
/// landing before this device's own echo goes out would otherwise overwrite
/// it, and for those, nothing else carries the change.
pub fn note_local_input<R: Runtime>(app: &AppHandle<R>) {
    app.state::<LuxNudge>().note_local_input();
}

/// Record edits this device made itself (input, a scene fade, the Discord
/// rail) as pending until an echo carries them (see [`Pending`]). Recorded
/// with or without a connection: that is how an edit made offline reaches
/// the rig once one returns.
pub fn note_local_edits<R: Runtime>(
    app: &AppHandle<R>,
    edits: impl IntoIterator<Item = (u16, u8)>,
) {
    app.state::<LuxNudge>()
        .pending
        .lock_or_recover()
        .record(edits, SystemTime::now());
}

/// [`note_local_edits`] for an overlay write (slot N takes `bytes[N - 1]`).
pub fn note_local_overlay<R: Runtime>(app: &AppHandle<R>, bytes: &[u8]) {
    note_local_edits(app, overlay_edits(bytes));
}

/// The active setup changed and the buffer is blanked for it. The live
/// connection republishes its presence card, so remote surfaces learn the new
/// binding without a reconnect. It also subscribes the new setup's `state`
/// topic, so this device takes in what that setup's rig is doing. Called
/// after the blank, so the echo it brings can't be wiped by it. No-op when no
/// connection is up.
pub fn active_setup_changed<R: Runtime>(app: &AppHandle<R>) {
    app.state::<LuxNudge>().presence.send_modify(|g| *g += 1);
}

/// Schedule a state-echo publish of the current buffer to the active setup's
/// `state` topic, trailing-edge coalesced to [`ECHO_WINDOW`]. Called from the
/// buffer's commit path, so local input and remotely-applied frames both
/// refresh the echo; fast no-op when no connection is live. The echo is never
/// fed back into a publish path, so it cannot loop.
pub fn schedule_state_echo<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LuxNudge>();
    if state.echo.lock_or_recover().is_none() {
        return;
    }
    // The echo carries the setup the change was made on, and the switch count
    // then, so a switch inside the window drops it rather than landing it on
    // the new setup.
    let queued = (
        app.state::<LuxSetups>().active_id().to_string(),
        state.switches.load(Ordering::SeqCst),
    );
    if state.echo_for.lock_or_recover().replace(queued).is_some() {
        return; // a publish is already queued and will pick this change up
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(ECHO_WINDOW).await;
        let queued = app.state::<LuxNudge>().echo_for.lock_or_recover().take();
        let Some((setup_id, switches)) = queued else {
            return;
        };
        publish_echo(&app, &setup_id, switches).await;
    });
}

/// Publish the buffer as `setup_id`'s state echo, if no setup switch happened
/// since it was queued (`switches`) and this connection holds the setup's
/// state. It is retained when the per-topic quota allows; otherwise it goes
/// out live and a trailing retained echo is queued.
async fn publish_echo<R: Runtime>(app: &AppHandle<R>, setup_id: &str, switches: u64) {
    let state = app.state::<LuxNudge>();
    let Some(echo) = state.echo.lock_or_recover().clone() else {
        return;
    };
    // Checked and snapshotted under the desk, so a switch can't land between
    // the checks and the read and put the blank on the old setup's topic.
    let buffer: Vec<u8> = {
        let _desk = state.desk.lock_or_recover();
        if state.switches.load(Ordering::SeqCst) != switches
            || app.state::<LuxSetups>().active_id().to_string() != setup_id
        {
            return; // queued for a setup that is no longer the one on the desk
        }
        if !matches!(
            state.held(&echo.session, setup_id),
            Some(Grip::Assumed | Grip::Firm)
        ) {
            // The buffer is only what this device last knew, or a first guess
            // that nothing is stored, and a node seeds from every full-buffer
            // echo. Input reaches the rig as frames meanwhile, and edits stay
            // pending until the state is in.
            return;
        }
        // Publishing makes an assumed hold firm: whatever the broker stored
        // before is older than this.
        state.hold(&echo.session, setup_id, Grip::Firm);
        app.state::<LuxBuffer>().buffer.lock_or_recover().clone()
    };
    let retain = state.take_retain_slot(setup_id);
    let frame = lux_wire::ctl::Frame::buffer(buffer).with_src(&state.publisher);
    let Ok(payload) = serde_json::to_vec(&frame) else {
        return;
    };
    let topic = lux_wire::ctl::state_topic(&echo.sub, setup_id);
    if let Err(e) = echo
        .client
        .publish(topic, QoS::AtMostOnce, retain, payload)
        .await
    {
        log::debug!("state echo publish failed (connection likely down): {e}");
        return;
    }
    if !retain {
        queue_trailing_retain(app, setup_id, switches);
    }
}

/// Queue one retained echo for when the quota next allows, so the stored copy
/// ends on whatever the buffer holds then: the final state of a drag or fade.
fn queue_trailing_retain<R: Runtime>(app: &AppHandle<R>, setup_id: &str, switches: u64) {
    let key = (setup_id.to_owned(), switches);
    {
        let state = app.state::<LuxNudge>();
        let mut queued = state.retain_queued.lock_or_recover();
        if queued.as_ref() == Some(&key) {
            return; // one is queued and will publish the latest buffer
        }
        *queued = Some(key.clone());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (setup_id, switches) = key;
        loop {
            let wait = app.state::<LuxNudge>().retain_wait(&setup_id);
            if wait.is_zero() {
                break;
            }
            tokio::time::sleep(wait).await;
        }
        {
            let state = app.state::<LuxNudge>();
            let mut queued = state.retain_queued.lock_or_recover();
            if queued
                .as_ref()
                .is_some_and(|(id, n)| *id == setup_id && *n == switches)
            {
                *queued = None;
            }
        }
        publish_echo(&app, &setup_id, switches).await;
    });
}

/// Work out, in two steps, that `setup_id` has no stored echo, if no retained
/// echo arrives after the broker acks its subscription. After one
/// [`LEARN_GRACE`] the hold becomes [`Grip::Asking`]: the topic is asked for
/// again, since a delivery can go missing, and pending edits go out as frames
/// but nothing full-buffer does. After a second, [`Grip::Assumed`]: nothing is
/// stored, the buffer stands, and pending edits are echoed. A real echo at
/// any point is still learned. Only the latest timer counts, so a switch
/// away and back can't conclude early.
fn arm_learn_grace(app: &AppHandle, session: &str, setup_id: &str) {
    let epoch = app
        .state::<LuxNudge>()
        .learn_epoch
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    let app = app.clone();
    let session = session.to_owned();
    let setup_id = setup_id.to_owned();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(LEARN_GRACE).await;
        if !grace_step(&app, epoch, &session, &setup_id, None, Grip::Asking) {
            return;
        }
        tokio::time::sleep(LEARN_GRACE).await;
        grace_step(
            &app,
            epoch,
            &session,
            &setup_id,
            Some(Grip::Asking),
            Grip::Assumed,
        );
    });
}

/// One step of [`arm_learn_grace`]: if the timer is still current and the
/// hold is still at `from`, move it to `to` and act on it. Returns whether it
/// moved.
fn grace_step(
    app: &AppHandle,
    epoch: u64,
    session: &str,
    setup_id: &str,
    from: Option<Grip>,
    to: Grip,
) -> bool {
    let state = app.state::<LuxNudge>();
    let _desk = state.desk.lock_or_recover();
    let echo = state.echo.lock_or_recover().clone();
    let current = state.learn_epoch.load(Ordering::SeqCst) == epoch
        && echo.as_ref().is_some_and(|e| e.session == session)
        && app.state::<LuxSetups>().active_id().to_string() == setup_id;
    if !current || state.held(session, setup_id) != from {
        return false;
    }
    state.hold(session, setup_id, to);
    match to {
        Grip::Asking => {
            let edits = state.pending.lock_or_recover().live(SystemTime::now());
            queue_edits(app, &edits, false);
            if let Some(echo) = echo {
                // A fresh subscription is sent the stored echo again.
                let topic = lux_wire::ctl::state_topic(&echo.sub, setup_id);
                if let Err(e) = echo.client.try_unsubscribe(topic.clone()) {
                    log::debug!("could not drop the state subscription to ask again: {e}");
                } else if let Err(e) = echo.client.try_subscribe(topic, QoS::AtMostOnce) {
                    log::debug!("could not ask for the stored state again: {e}");
                }
            }
        }
        Grip::Assumed | Grip::Firm => {
            if !state.pending.lock_or_recover().is_empty() {
                schedule_state_echo(app);
            }
        }
    }
    true
}

/// Queue edits this device made while it couldn't send them, as frames, so
/// every applier applies just those slots. Many at once (a scene recalled
/// offline can touch the whole patch) would run past the broker's
/// per-connection publish rate as channel frames. When the buffer is known
/// ground (`learned`), they go as one overlay of its leading slots instead.
/// Otherwise an overlay would carry the slots between them from a buffer
/// that learned nothing, so they wait for the echo that follows once the
/// state is known. Called under the desk, so a setup switch can't fall
/// between and send them to the next setup.
fn queue_edits<R: Runtime>(app: &AppHandle<R>, edits: &[(u16, u8)], learned: bool) {
    const DENSE: usize = 32;
    let Some(&(last, _)) = edits.last() else {
        return;
    };
    let dense = edits.len() > DENSE;
    if dense && !learned {
        return;
    }
    {
        let state = app.state::<LuxNudge>();
        let mut outbox = state.outbox.lock_or_recover();
        if dense {
            let mut leading = app.state::<LuxBuffer>().buffer.lock_or_recover().clone();
            leading.truncate(usize::from(last));
            outbox.push_overlay(leading);
        } else {
            for &(slot, val) in edits {
                outbox.push_channel(slot, val);
            }
        }
    }
    schedule_publish(app);
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
/// buffer, not the outbox. The caller notes the write as pending before making
/// it ([`note_local_overlay`]), so with the user channel down this is a no-op
/// and the edit goes out when a connection returns.
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
/// publish. Same contract as [`publish_input_overlay`], with
/// [`note_local_edits`] before the write.
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
/// the setup active at flush time, stamped with this process's publisher id
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
            // Connection died inside the window; drop the batch. Its edits are
            // still pending, so the reconnect delivers them on top of the
            // setup's state.
            state.outbox.lock_or_recover().drain();
            return;
        };
        // Drained and addressed under the desk: a setup switch drains the
        // outbox there too, so frames queued for one setup can't go out to the
        // next.
        let (frames, setup_id) = {
            let _desk = state.desk.lock_or_recover();
            let frames = state.outbox.lock_or_recover().drain();
            (frames, app.state::<LuxSetups>().active_id().to_string())
        };
        let topic = lux_wire::ctl::frame_topic(&echo.sub, &setup_id);
        for frame in frames {
            let Ok(payload) = serde_json::to_vec(&frame.with_src(&state.publisher)) else {
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
    // The suffix doubles as the connection's ctl session id: it names the
    // presence topic and keys what the connection holds of a setup's state.
    // Outgoing frames carry the per-process publisher id instead (`Frame::src`).
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
    // Later acks (the active setup's state topic, shared-control grants) need
    // nothing.
    let mut acks = 0u32;
    // The setup `state` topic this connection holds by name (see
    // [`resubscribe_state`]), whether the active setup's is owed (after the
    // ctl ack, a switch, or a subscribe the client couldn't queue), and the
    // setup whose subscribe awaits its ack — the grace timer starts there.
    let mut held_state: Option<String> = None;
    let mut want_state = false;
    let mut awaiting_ack: Option<String> = None;
    // A presence card the client couldn't queue, owed on the next pass.
    let mut want_presence = false;

    loop {
        if want_presence && publish_presence(&client, app, sub, &session) {
            want_presence = false;
        }
        if want_state {
            let active = app.state::<LuxSetups>().active_id().to_string();
            if resubscribe_state(&client, sub, &active, &mut held_state) {
                want_state = false;
                awaiting_ack = Some(active);
            }
        }
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
                    want_presence = !publish_presence(&client, app, sub, &session);
                    want_state = true;
                }
            }
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    if let Some(setup_id) = awaiting_ack.take() {
                        arm_learn_grace(app, &session, &setup_id);
                    }
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
                        want_presence = !publish_presence(&client, app, sub, &session);
                        // Take in the setup's retained echo and publish no
                        // echo until then: this buffer is only what the device
                        // last knew, and a node seeds from every echo it
                        // hears. Edits made meanwhile, or while the channel
                        // was down, are pending and go out on top of it.
                        want_state = true;
                        // Shared-control guests see a setup only through its
                        // retained config. Grants can change while this device
                        // is offline, so reconcile on every connect rather
                        // than trusting what a past session published.
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
                            apply_frame(app, &publish.payload, setup_id);
                        }
                        Route::State { setup_id } => {
                            reflect_state(
                                app,
                                &publish.payload,
                                setup_id,
                                &session,
                                publish.retain,
                            );
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
///
/// It never waits on the client's request channel: the callers run inside the
/// event loop's `select!`, which is what drains that channel. Returns whether
/// the card was queued; the caller tries again on its next pass if not.
fn publish_presence(client: &AsyncClient, app: &AppHandle, sub: &str, session: &str) -> bool {
    let setup_id = app.state::<LuxSetups>().active_id().to_string();
    let card = lux_wire::ctl::PresenceCard::new(session.to_owned(), setup_id, device_name());
    let Ok(payload) = serde_json::to_vec(&card) else {
        return true; // nothing a retry would fix
    };
    let topic = lux_wire::ctl::presence_topic(sub, session);
    match client.try_publish(topic, QoS::AtMostOnce, true, payload) {
        Ok(()) => true,
        Err(e) => {
            log::debug!("presence publish failed: {e}");
            false
        }
    }
}

/// Subscribe `setup_id`'s `state` topic by name, dropping the one `held`
/// before, so the broker hands over that setup's retained echo. AWS IoT Core
/// never delivers a retained message through a wildcard, so the ctl wildcard
/// alone would leave this device with only what it last knew. Unsubscribing
/// first makes a return to an earlier setup a fresh subscription, which is
/// what gets the retained echo sent again. Neither request waits on the
/// client's channel (see [`publish_presence`]). Returns whether the subscribe
/// was queued; the caller tries again on its next pass if not.
fn resubscribe_state(
    client: &AsyncClient,
    sub: &str,
    setup_id: &str,
    held: &mut Option<String>,
) -> bool {
    if let Some(old) = held.take() {
        if let Err(e) = client.try_unsubscribe(old.clone()) {
            log::debug!("could not drop the previous state subscription: {e}");
            *held = Some(old);
            return false;
        }
    }
    let topic = lux_wire::ctl::state_topic(sub, setup_id);
    match client.try_subscribe(topic.clone(), QoS::AtMostOnce) {
        Ok(()) => {
            *held = Some(topic);
            true
        }
        Err(e) => {
            log::debug!("could not subscribe to {topic}: {e}");
            false
        }
    }
}

/// This device's human-readable name for presence cards.
pub(crate) fn device_name() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// Reflect an applier's state echo into the buffer and the UI/persistence,
/// **without publishing or re-echoing** — remote state must never re-enter
/// the publish paths, which is what makes two devices echoing at each other
/// impossible by construction. What it does depends on [`landing`]: the first
/// echo a connection takes in for the setup becomes the base its pending
/// edits go back on top of, and is rendered; later ones overwrite.
///
/// `retained` is the MQTT retain flag as delivered: set only on the broker's
/// stored copy, sent because this connection subscribed the topic.
fn reflect_state(
    app: &AppHandle,
    payload: &[u8],
    frame_setup: &str,
    own_session: &str,
    retained: bool,
) {
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
    let state = app.state::<LuxNudge>();
    let own = frame.src() == Some(state.publisher.as_str());
    let lux_wire::ctl::Frame::Buffer { mut buffer, .. } = frame else {
        log::debug!("state echo carried a non-buffer frame; ignoring");
        return;
    };
    // Under the desk, so a setup switch can't land between the check that this
    // echo is for the active setup and the write that takes it in.
    let _desk = state.desk.lock_or_recover();
    let active = app.state::<LuxSetups>().active_id().to_string();
    if frame_setup != active {
        return;
    }
    // Whatever this echo carries is on the wire, whoever sent it and whether or
    // not it lands: those edits are confirmed. Whether this process had it in
    // its last moments before going quiet is read first, for the learn below.
    state.pending.lock_or_recover().confirm(&buffer);
    let recent = {
        let mut seen = state.seen.lock_or_recover();
        let recent = seen.recent(&buffer);
        seen.note(&buffer);
        recent
    };
    if own && !retained {
        return; // our own echo back; confirming was all it brought
    }
    let grip = state.held(own_session, &active);
    match landing(retained, grip, state.within_reflect_holdoff()) {
        Landing::Skip => {
            log::trace!("not reflecting a state echo (retained: {retained}, held: {grip:?})");
        }
        Landing::Live => {
            // Superseded: anything still pending lost to a later change.
            state.pending.lock_or_recover().clear();
            crate::buffer::reflect_remote_state(app, &buffer, false);
        }
        Landing::Learn => {
            state.hold(own_session, &active, Grip::Firm);
            let current = app.state::<LuxBuffer>().buffer.lock_or_recover().clone();
            let kept = recent && !state.pending.lock_or_recover().is_empty();
            let edits = if kept {
                // The device made edits while it couldn't send them, and this
                // is the state it had just before going quiet: nothing moved
                // on the broker's side meanwhile. The buffer holds that state
                // and everything since, and stays whole; every pending edit in
                // it is still ours to deliver, however old. The holdoff keeps
                // a stale echo arriving just behind from undoing that.
                state.note_local_input();
                state.pending.lock_or_recover().all()
            } else {
                // Someone changed the setup while this device couldn't hear,
                // or it has nothing of its own to protect. The stored state
                // stands, with this device's recent edits on top.
                let edits = state.pending.lock_or_recover().live(SystemTime::now());
                for &(slot, val) in &edits {
                    if let Some(level) = usize::from(slot)
                        .checked_sub(1)
                        .and_then(|index| buffer.get_mut(index))
                    {
                        *level = val;
                    }
                }
                // Rendered: a USB output holds its last frame, and after a
                // switch the blank would otherwise stay up until the keepalive.
                crate::buffer::reflect_remote_state(app, &buffer, true);
                edits
            };
            // What this device changed while it couldn't send it goes out as
            // frames, then an echo of the state they sit on — unless the
            // stored echo already is that state.
            queue_edits(app, &edits, true);
            let ahead = if kept {
                current != buffer
            } else {
                !edits.is_empty()
            };
            if ahead {
                schedule_state_echo(app);
            }
        }
    }
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
fn apply_frame(app: &AppHandle, payload: &[u8], frame_setup: &str) {
    let frame: lux_wire::ctl::Frame = match serde_json::from_slice(payload) {
        Ok(frame) => frame,
        Err(e) => {
            log::warn!("ignoring unreadable ctl frame: {e}");
            return;
        }
    };
    let state = app.state::<LuxNudge>();
    let active = app.state::<LuxSetups>().active_id().to_string();
    let Some(apply) = gate(frame, frame_setup, &active, &state.publisher) else {
        return;
    };
    let mut buffer = app.state::<LuxBuffer>().inner().clone();
    // Writes happen under the desk, re-checking the setup there, so a frame for
    // the setup being left can't land on the next one's blank.
    let still_active = || app.state::<LuxSetups>().active_id().to_string() == frame_setup;
    let result = match apply {
        RemoteApply::Overlay(bytes) => {
            let _desk = state.desk.lock_or_recover();
            if !still_active() {
                return;
            }
            state
                .pending
                .lock_or_recover()
                .supersede(overlay_edits(&bytes).map(|(slot, _)| slot));
            buffer.set(bytes, app.clone()).map(|_| ())
        }
        RemoteApply::Channel { ch, val } => {
            let _desk = state.desk.lock_or_recover();
            if !still_active() {
                return;
            }
            state.pending.lock_or_recover().supersede([ch]);
            buffer
                .set_channel(usize::from(ch), val, app.clone())
                .map(|_| ())
        }
        // A recall resolves against the setup this applier holds and runs the
        // ordinary recall path (fade included) — a remote scene press behaves
        // exactly like pressing the button here. An id we don't know means the
        // sender's config was stale; drop it and let the next retained config
        // publish correct them.
        RemoteApply::Scene { id } => uuid::Uuid::parse_str(&id)
            .map_err(|e| format!("unparseable scene id {id}: {e}"))
            .and_then(|id| {
                app.state::<LuxSetups>()
                    .active_scene(id)
                    .ok_or_else(|| format!("scene {id} not on the active setup"))
            })
            .and_then(|scene| crate::scene::recall(app, &scene)),
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
            scenes: Vec::new(),
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

    #[test]
    fn the_first_echo_on_a_connection_is_learned() {
        // Nothing held yet: this device has only what it last knew, so the
        // rig's state is taken in — retained or live, hand on the desk or not.
        for retained in [true, false] {
            for within_holdoff in [true, false] {
                assert_eq!(landing(retained, None, within_holdoff), Landing::Learn);
            }
        }
    }

    #[test]
    fn any_real_echo_beats_the_grace_timers_guess() {
        // Asking or assuming, the grace timer only guessed nothing is stored:
        // a late retained echo, or the first live one, is learned.
        for guess in [Grip::Asking, Grip::Assumed] {
            for retained in [true, false] {
                for within_holdoff in [true, false] {
                    assert_eq!(
                        landing(retained, Some(guess), within_holdoff),
                        Landing::Learn
                    );
                }
            }
        }
    }

    #[test]
    fn once_firm_a_stored_copy_is_skipped_and_live_echoes_respect_the_holdoff() {
        // A stored copy can trail live traffic: never newer than a firm hold.
        assert_eq!(landing(true, Some(Grip::Firm), false), Landing::Skip);
        assert_eq!(landing(true, Some(Grip::Firm), true), Landing::Skip);

        assert_eq!(landing(false, Some(Grip::Firm), false), Landing::Live);
        assert_eq!(landing(false, Some(Grip::Firm), true), Landing::Skip);
    }

    #[test]
    fn a_hold_belongs_to_one_connection_and_one_setup() {
        let state = LuxNudge::default();
        assert_eq!(state.held("conn-1", "s-1"), None);

        state.hold("conn-1", "s-1", Grip::Asking);
        assert_eq!(state.held("conn-1", "s-1"), Some(Grip::Asking));
        state.hold("conn-1", "s-1", Grip::Firm);
        assert_eq!(state.held("conn-1", "s-1"), Some(Grip::Firm));

        // A reconnect, or another setup, holds nothing.
        assert_eq!(state.held("conn-2", "s-1"), None);
        assert_eq!(state.held("conn-1", "s-2"), None);
    }

    #[test]
    fn pending_edits_keep_only_real_slots() {
        let now = SystemTime::now();
        let mut pending = Pending::default();
        pending.record([(0, 1), (513, 1), (512, 9)], now);
        assert_eq!(pending.live(now), vec![(512, 9)]);

        // An oversized overlay keeps its first 512 slots and drops the rest,
        // which no echo could ever confirm.
        let mut pending = Pending::default();
        pending.record(overlay_edits(&[7u8; 600]), now);
        assert_eq!(pending.live(now).len(), UNIVERSE_SIZE);
        assert!(!pending.is_empty());
    }

    #[test]
    fn pending_edits_are_confirmed_by_any_echo_that_carries_them() {
        let now = SystemTime::now();
        let mut pending = Pending::default();
        pending.record([(1, 10), (3, 30)], now);
        pending.record(overlay_edits(&[11]), now); // slot 1 again, newer value

        let mut echo = vec![0u8; 4];
        echo[0] = 10; // the old value for slot 1: not ours any more
        echo[2] = 30; // slot 3 arrived
        pending.confirm(&echo);
        assert_eq!(pending.live(now), vec![(1, 11)]);

        echo[0] = 11;
        pending.confirm(&echo);
        assert!(pending.live(now).is_empty());
        assert_eq!(pending.at, None);
    }

    #[test]
    fn a_peers_later_frame_supersedes_a_pending_edit() {
        let now = SystemTime::now();
        let mut pending = Pending::default();
        pending.record([(1, 10), (2, 20)], now);
        pending.supersede([1]);
        assert_eq!(pending.live(now), vec![(2, 20)]);
    }

    #[test]
    fn pending_edits_expire_after_their_lifetime() {
        let then = SystemTime::now();
        let mut pending = Pending::default();
        pending.record([(5, 50)], then);
        assert_eq!(pending.live(then + PENDING_LIFETIME), vec![(5, 50)]);
        assert!(pending
            .live(then + PENDING_LIFETIME + Duration::from_secs(1))
            .is_empty());

        // A clock that ran backwards doesn't expire anything.
        pending.record([(5, 50)], then);
        assert_eq!(pending.live(then - Duration::from_secs(60)), vec![(5, 50)]);
    }

    #[test]
    fn an_overlay_edits_its_leading_slots_one_based() {
        assert_eq!(
            overlay_edits(&[7, 8, 9]).collect::<Vec<_>>(),
            vec![(1, 7), (2, 8), (3, 9)]
        );
        assert_eq!(overlay_edits(&[]).count(), 0);
    }

    #[test]
    fn retained_echoes_are_paced_per_setup() {
        let state = LuxNudge::default();
        assert!(state.take_retain_slot("s-1"), "nothing retained yet");
        assert!(!state.take_retain_slot("s-1"), "inside the interval");
        assert!(state.retain_wait("s-1") > Duration::ZERO);

        // The quota is per topic: another setup's echo has its own, and a
        // switch back finds its own pacing where it left it.
        assert!(state.take_retain_slot("s-2"));
        assert!(!state.take_retain_slot("s-1"));
        assert!(state.retain_wait("s-1") > Duration::ZERO);
    }

    #[test]
    fn a_stored_echo_is_recognized_by_content_not_by_who_sent_it() {
        let t0 = Instant::now();
        let mut seen = Seen::default();
        let ours = vec![1u8; UNIVERSE_SIZE];
        let mut later = ours.clone();
        later[4] = 200;

        seen.note_at(&ours, t0);
        seen.note_at(&later, t0 + Duration::from_millis(300));
        // The node echoing our state carries the same bytes, so whichever of
        // the two echoes the broker kept is recognized.
        assert!(seen.recent(&ours));
        assert!(seen.recent(&later));

        // A state changed by someone while we weren't listening is not.
        let mut theirs = later.clone();
        theirs[9] = 7;
        assert!(!seen.recent(&theirs));

        seen.clear();
        assert!(!seen.recent(&ours));
    }

    #[test]
    fn a_state_from_before_the_last_moments_is_somebody_elses_new_change() {
        // Scene X, then scene Y well after. If the rig is back at X when we
        // return, someone recalled it again; X is not what we last had.
        let t0 = Instant::now();
        let mut seen = Seen::default();
        let x = vec![10u8; UNIVERSE_SIZE];
        let y = vec![20u8; UNIVERSE_SIZE];
        seen.note_at(&x, t0);
        seen.note_at(&y, t0 + Seen::RECENT + Duration::from_secs(1));
        assert!(!seen.recent(&x));
        assert!(seen.recent(&y));

        // Noting X again refreshes it as the newest.
        seen.note_at(&x, t0 + Seen::RECENT * 3);
        assert!(seen.recent(&x));
        assert!(!seen.recent(&y));
    }

    #[test]
    fn the_seen_window_keeps_the_newest_echoes() {
        let t0 = Instant::now();
        let mut seen = Seen::default();
        let echo = |n: usize| {
            let mut buffer = vec![0u8; UNIVERSE_SIZE];
            buffer[0] = u8::try_from(n % 256).unwrap();
            buffer[1] = u8::try_from(n / 256).unwrap();
            buffer
        };
        for n in 0..=Seen::WINDOW {
            seen.note_at(&echo(n), t0);
        }
        assert!(!seen.recent(&echo(0)), "the oldest fell out");
        assert!(seen.recent(&echo(1)));
        assert!(seen.recent(&echo(Seen::WINDOW)));

        // A repeat of the newest takes no room.
        seen.note_at(&echo(Seen::WINDOW), t0);
        assert!(seen.recent(&echo(1)));
    }
}
