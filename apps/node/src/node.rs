//! The node's run loop: one user-channel connection, frames applied to a
//! plain universe buffer, sACN out.
//!
//! Mirrors the desktop listener's connection shape (`nudge.rs`) minus Tauri:
//! WSS to IoT Core through the `lux-sync-auth` custom authorizer with a fresh
//! Cognito ID token per attempt, a retained presence card cleared by the Last
//! Will, and capped exponential backoff. Differences fitting a render node:
//! it subscribes only its setup's `frame` and `state` topics (a node never
//! syncs). Each connection takes in the setup's retained state echo before it
//! applies frames or publishes anything, which is what restores the look after
//! a restart. It transmits nothing until it holds a look, then re-renders
//! every second, reconnects included (sACN receivers drop a source that goes
//! quiet). It echoes its state only after a frame changes it.

use std::cell::Cell;
use std::future::Future;
use std::time::{Duration, Instant};

use lux_engine::ctl::{gate, retain_wait, route, RemoteApply, Route};
use lux_engine::sacn::SacnSink;
use lux_engine::tls::webpki_pem_bundle;
use lux_engine::universe::Universe;
use lux_engine::DmxSink;
use rumqttc::{
    AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Packet, QoS, SubscribeFilter,
    TlsConfiguration, Transport,
};
use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::time::Interval;
use uuid::Uuid;

use crate::auth;
use crate::config::{Endpoints, NodeConfig, StoredSession};

/// sACN keepalive: receivers time a source out after ~2.5s of silence.
const KEEPALIVE: Duration = Duration::from_secs(1);
/// Trailing-edge coalescing for the state echo (≤5 Hz).
const ECHO_WINDOW: Duration = Duration::from_millis(200);
/// How long after the subscribe ack to wait for the setup's retained echo
/// before asking again, and after that before concluding there is none.
const RESTORE_GRACE: Duration = Duration::from_millis(1500);
/// A failing send is retried at every keepalive, so an outage would log a line
/// a second; after the first, failures are reported at most this often.
const SEND_FAILURE_REPORT: Duration = Duration::from_secs(60);

/// What the node holds on the rig. Outlives any one connection, so a reconnect
/// keeps the look, and a change not yet announced still goes out unless a
/// stored state from someone else replaces it.
struct Rig {
    universe: Universe,
    /// The universe holds a look: seeded from a state echo or set by a frame.
    /// Nothing is transmitted until then, so a restart doesn't flash black
    /// ahead of the retained echo it restores from.
    lit: bool,
    /// The universe stands on known ground: seeded from an echo, kept over the
    /// node's own stored one, or announced by the node itself. Until then a
    /// stored echo that arrives late still seeds it, with frames applied in
    /// the meantime replayed on top.
    restored: bool,
    /// A frame changed the universe since the last state echo went out.
    echo_dirty: bool,
    /// Echoes have gone out live since the last retained one, so the stored
    /// copy trails the universe (see [`lux_engine::ctl::RETAIN_INTERVAL`]).
    retained_behind: bool,
    /// When this process last published a retained echo.
    last_retained: Option<Instant>,
    /// Stamped on every state echo this process publishes. Per process, not
    /// per connection like the client-id session: the universe outlives a
    /// connection, so the node's retained echo from an earlier connection must
    /// still read as its own. It holds nothing newer than the universe does,
    /// and seeding from it would undo a change whose echo never got out.
    src: String,
    /// The run of failed sends under way, if any (see [`track_send`]).
    send_failures: Cell<Option<FailureRun>>,
}

/// Consecutive failed sends, counted so they are reported at a bounded rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FailureRun {
    /// Failed sends since the run began.
    total: u64,
    /// Failed sends since the last report.
    unreported: u64,
    /// When the run was last reported.
    reported_at: Instant,
}

/// What one send result adds to the log.
#[derive(Debug, PartialEq, Eq)]
enum SendReport {
    /// Nothing new to say.
    Quiet,
    /// The first failure of a run.
    Failed,
    /// Still failing: this many more failures since the last report.
    StillFailing(u64),
    /// Sending works again after this many failures.
    Recovered(u64),
}

/// Fold one send result into the run of failures. Pure, so the rate limit is
/// unit-tested.
fn track_send(
    run: Option<FailureRun>,
    failed: bool,
    now: Instant,
) -> (Option<FailureRun>, SendReport) {
    match (run, failed) {
        (None, false) => (None, SendReport::Quiet),
        (Some(run), false) => (None, SendReport::Recovered(run.total)),
        (None, true) => (
            Some(FailureRun {
                total: 1,
                unreported: 0,
                reported_at: now,
            }),
            SendReport::Failed,
        ),
        (Some(run), true) => {
            let total = run.total.saturating_add(1);
            let unreported = run.unreported.saturating_add(1);
            if now.saturating_duration_since(run.reported_at) >= SEND_FAILURE_REPORT {
                let run = FailureRun {
                    total,
                    unreported: 0,
                    reported_at: now,
                };
                (Some(run), SendReport::StillFailing(unreported))
            } else {
                let run = FailureRun {
                    total,
                    unreported,
                    ..run
                };
                (Some(run), SendReport::Quiet)
            }
        }
    }
}

impl Rig {
    /// Send the look again, if there is one: receivers hold it only while it
    /// keeps arriving. A failing send is reported once, then at most every
    /// [`SEND_FAILURE_REPORT`] while it keeps failing, and again when it
    /// recovers.
    fn transmit(&self, sink: &SacnSink) {
        if !self.lit {
            return;
        }
        let sent = sink.render(self.universe.slots());
        let (run, report) = track_send(self.send_failures.get(), sent.is_err(), Instant::now());
        self.send_failures.set(run);
        match (report, sent) {
            (SendReport::Failed, Err(e)) => log::warn!("sACN render failed: {e}"),
            (SendReport::StillFailing(n), Err(e)) => {
                log::warn!("sACN render still failing ({n} more failed sends): {e}");
            }
            (SendReport::Recovered(n), _) => {
                log::info!("sACN render recovered after {n} failed sends");
            }
            _ => {}
        }
    }

    /// Apply one gated frame; returns whether the universe changed.
    fn apply(&mut self, apply: RemoteApply) -> bool {
        match apply {
            RemoteApply::Overlay(bytes) => {
                self.universe.overlay(&bytes);
                true
            }
            RemoteApply::Channel { ch, val } => match self.universe.set_slot(ch, val) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("ctl frame apply failed: {e}");
                    false
                }
            },
            // The node holds no scenes — only the desktop applier can resolve a
            // recall. Its state echo carries the resulting levels here instead.
            RemoteApply::Scene { id } => {
                log::info!("ignoring a scene recall ({id}): the node holds no scenes");
                false
            }
        }
    }

    /// Apply the frames held while a connection settled; returns whether any
    /// changed the universe.
    fn replay(&mut self, held: Vec<RemoteApply>) -> bool {
        let mut changed = false;
        for apply in held {
            changed |= self.apply(apply);
        }
        changed
    }
}

pub async fn run(
    env: Endpoints,
    cfg: NodeConfig,
    session: StoredSession,
    sink: SacnSink,
) -> Result<(), String> {
    let mut rig = Rig {
        universe: Universe::default(),
        lit: false,
        restored: false,
        echo_dirty: false,
        retained_behind: false,
        last_retained: None,
        src: Uuid::new_v4().simple().to_string()[..8].to_owned(),
        send_failures: Cell::new(None),
    };
    let mut refresh_token = session.refresh_token;
    let client_id = session.client_id;
    let mut backoff_secs = 1u64;
    let mut sigterm =
        signal(SignalKind::terminate()).map_err(|e| format!("sigterm handler: {e}"))?;
    // Between connections, where no connection's keepalive is running.
    let mut keepalive = tokio::time::interval(KEEPALIVE);

    let shutdown = |sink: &SacnSink, universe: &Universe| {
        log::info!("shutting down; sending stream-terminated packets");
        sink.terminate(universe.slots());
    };

    loop {
        // Fresh ID token every attempt; Cognito may rotate the refresh token.
        let refresh = auth::refresh(&env, client_id.as_deref(), &refresh_token);
        let Some(refreshed) =
            holding_look(refresh, &sink, &rig, &mut keepalive, &mut sigterm).await
        else {
            shutdown(&sink, &rig.universe);
            return Ok(());
        };
        let tokens = match refreshed {
            Ok(tokens) => tokens,
            Err(e) => {
                log::warn!("token refresh failed ({e}); retrying in {backoff_secs}s");
                let pause = tokio::time::sleep(Duration::from_secs(backoff_secs));
                if holding_look(pause, &sink, &rig, &mut keepalive, &mut sigterm)
                    .await
                    .is_none()
                {
                    shutdown(&sink, &rig.universe);
                    return Ok(());
                }
                backoff_secs = (backoff_secs * 2).min(30);
                continue;
            }
        };
        if let Some(rotated) = &tokens.refresh {
            refresh_token = rotated.clone();
        }
        let Some(sub) = lux_engine::auth::jwt_sub(&tokens.id) else {
            return Err("could not read sub from the id token".into());
        };

        let terminated = run_connection(
            &env,
            &cfg,
            &sink,
            &mut rig,
            &sub,
            tokens.id,
            &mut backoff_secs,
            &mut sigterm,
        )
        .await;

        if terminated {
            shutdown(&sink, &rig.universe);
            return Ok(());
        }

        let pause = tokio::time::sleep(Duration::from_secs(backoff_secs));
        if holding_look(pause, &sink, &rig, &mut keepalive, &mut sigterm)
            .await
            .is_none()
        {
            shutdown(&sink, &rig.universe);
            return Ok(());
        }
        backoff_secs = (backoff_secs * 2).min(30);
    }
}

/// Await `work` while keeping the look on the wire. A reconnect (token
/// refresh, backoff) can outlast the ~2.5 s a receiver waits before dropping a
/// quiet source. `None` when SIGTERM arrives first.
async fn holding_look<T>(
    work: impl Future<Output = T>,
    sink: &SacnSink,
    rig: &Rig,
    keepalive: &mut Interval,
    sigterm: &mut Signal,
) -> Option<T> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            out = &mut work => return Some(out),
            _ = keepalive.tick() => rig.transmit(sink),
            _ = sigterm.recv() => return None,
        }
    }
}

#[allow(clippy::too_many_arguments)] // one call site; a struct would be noise
async fn run_connection(
    env: &Endpoints,
    cfg: &NodeConfig,
    sink: &SacnSink,
    rig: &mut Rig,
    sub: &str,
    token: String,
    backoff_secs: &mut u64,
    sigterm: &mut Signal,
) -> bool {
    // Random per-session suffix: shares the peers' client-id prefix (the
    // authorizer allows it) and names this connection's presence card.
    let session = Uuid::new_v4().simple().to_string()[..8].to_owned();
    let client_id = format!("{}{}", lux_wire::nudge::client_id_prefix(sub), session);
    let presence_topic = lux_wire::ctl::presence_topic(sub, &session);
    let state_topic = lux_wire::ctl::state_topic(sub, &cfg.setup_id);
    let url = format!(
        "wss://{}/mqtt?x-amz-customauthorizer-name={}",
        env.nudge_endpoint,
        lux_wire::nudge::AUTHORIZER_NAME
    );
    let mut opts = MqttOptions::new(client_id, url, 443);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_last_will(LastWill::new(
        presence_topic.clone(),
        Vec::<u8>::new(),
        QoS::AtMostOnce,
        true,
    ));
    opts.set_transport(Transport::Wss(TlsConfiguration::Simple {
        ca: webpki_pem_bundle().to_vec(),
        alpn: None,
        client_auth: None,
    }));
    let header_token = token;
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
    let filters =
        subscriptions(sub, &cfg.setup_id).map(|topic| SubscribeFilter::new(topic, QoS::AtMostOnce));
    if let Err(e) = client.subscribe_many(filters).await {
        log::warn!("could not queue the ctl subscribe: {e}");
        return false;
    }

    let mut keepalive = tokio::time::interval(KEEPALIVE);
    let mut echo_tick = tokio::time::interval(ECHO_WINDOW);
    // When the subscribe was acked (or the state topic asked for again); the
    // setup's retained echo follows the ack.
    let mut acked_at: Option<Instant> = None;
    // Whether the state topic has been asked for a second time, in case the
    // first retained delivery went missing, and whether that left it
    // unsubscribed with the new subscribe still to be queued.
    let mut asked_again = false;
    let mut subscribe_owed = false;
    // Whether this connection has taken in the setup's state from the broker:
    // its retained echo, a live echo that beat it, or two grace periods with
    // neither. Until then frames are held rather than applied, since the
    // restore would overwrite them, and nothing is echoed.
    let mut settled = false;
    let mut held: Vec<RemoteApply> = Vec::new();
    // Frames applied after settling while the universe had nothing to stand
    // on (`Rig::restored`): a stored echo that turns up late goes under them.
    let mut unrestored: Vec<RemoteApply> = Vec::new();

    loop {
        tokio::select! {
            _ = keepalive.tick() => rig.transmit(sink),
            _ = echo_tick.tick() => {
                if subscribe_owed {
                    match client.try_subscribe(state_topic.clone(), QoS::AtMostOnce) {
                        Ok(()) => subscribe_owed = false,
                        Err(e) => log::debug!("could not subscribe to the state echo again: {e}"),
                    }
                }
                if settled {
                    publish_due_echo(&client, &state_topic, rig);
                    if rig.restored {
                        unrestored.clear();
                    }
                } else if acked_at.is_some_and(|at| at.elapsed() >= RESTORE_GRACE) {
                    if !asked_again {
                        // Ask once more before concluding there is no stored
                        // echo: a fresh subscription is sent it again.
                        asked_again = true;
                        acked_at = Some(Instant::now());
                        match client.try_unsubscribe(state_topic.clone()) {
                            // Subscribed again on the next tick at the latest:
                            // live echoes, scene recalls among them, arrive
                            // only through it.
                            Ok(()) => subscribe_owed = true,
                            Err(e) => log::debug!("could not drop the state subscription: {e}"),
                        }
                    } else {
                        // No retained echo came: nothing has echoed this setup yet.
                        settled = true;
                        let replay = std::mem::take(&mut held);
                        if !rig.restored {
                            unrestored.extend(replay.iter().cloned());
                        }
                        if rig.replay(replay) {
                            rig.lit = true;
                            rig.echo_dirty = true;
                            rig.transmit(sink);
                        }
                    }
                }
            }
            _ = sigterm.recv() => {
                return true;
            }
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    if acked_at.is_none() {
                        log::info!("user channel connected; applying setup {}", cfg.setup_id);
                        *backoff_secs = 1;
                        publish_presence(&client, &presence_topic, cfg, &session);
                    }
                    acked_at = Some(Instant::now());
                }
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    match route(&publish.topic, sub) {
                        Route::Frame { setup_id } if setup_id == cfg.setup_id => {
                            let Some(apply) = parse_frame(&publish.payload, cfg, &rig.src) else {
                                continue;
                            };
                            if !settled {
                                held.push(apply);
                                continue;
                            }
                            if !rig.restored {
                                unrestored.push(apply.clone());
                            }
                            if rig.apply(apply) {
                                rig.lit = true;
                                rig.echo_dirty = true;
                                rig.transmit(sink);
                            }
                        }
                        Route::State { setup_id }
                            if setup_id == cfg.setup_id =>
                        {
                            let ctx = EchoContext {
                                retained: publish.retain,
                                settled,
                                restored: rig.restored,
                            };
                            match read_echo(&publish.payload, &rig.src, ctx) {
                                EchoAction::Seed(buffer) => {
                                    // Another applier's state: the setup's
                                    // stored echo on connect, or a live one
                                    // after a change made there (a scene
                                    // recall's levels reach the node this
                                    // way). Seeded, never echoed back: an
                                    // applier that echoed what it seeded would
                                    // bounce it off any other applier forever.
                                    rig.universe.overlay(&buffer);
                                    // Frames that came after the stored copy
                                    // go on top: held while settling, or
                                    // applied before a late one turned up.
                                    let replay = if !settled {
                                        std::mem::take(&mut held)
                                    } else if !rig.restored {
                                        std::mem::take(&mut unrestored)
                                    } else {
                                        Vec::new()
                                    };
                                    let replayed = rig.replay(replay);
                                    settled = true;
                                    rig.lit = true;
                                    rig.restored = true;
                                    rig.echo_dirty = replayed;
                                    rig.transmit(sink);
                                    if publish.retain {
                                        log::info!("restored the universe from the retained state echo");
                                    } else {
                                        log::debug!("seeded the universe from a live state echo");
                                    }
                                }
                                EchoAction::Keep => {
                                    settled = true;
                                    rig.restored = true;
                                    if rig.replay(std::mem::take(&mut held)) {
                                        rig.lit = true;
                                        rig.echo_dirty = true;
                                        rig.transmit(sink);
                                    }
                                }
                                EchoAction::Ignore => {}
                            }
                        }
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    log::info!("connection error (will reconnect): {e}");
                    if matches!(e, ConnectionError::ConnectionRefused(_)) {
                        // Auth-shaped: the caller refreshes the token first.
                    }
                    return false;
                }
            }
        }
    }
}

/// The two topics a node acts on: its setup's live frames and its state echo.
/// Each is named exactly rather than through the ctl wildcard, because AWS IoT
/// Core delivers a retained message only to a subscription that names its
/// topic — and the retained state echo is what a restarted node seeds from.
fn subscriptions(sub: &str, setup_id: &str) -> [String; 2] {
    [
        lux_wire::ctl::frame_topic(sub, setup_id),
        lux_wire::ctl::state_topic(sub, setup_id),
    ]
}

/// Parse and gate one ctl frame for this node's setup.
fn parse_frame(payload: &[u8], cfg: &NodeConfig, src: &str) -> Option<RemoteApply> {
    let frame: lux_wire::ctl::Frame = match serde_json::from_slice(payload) {
        Ok(frame) => frame,
        Err(e) => {
            log::warn!("ignoring unreadable ctl frame: {e}");
            return None;
        }
    };
    gate(frame, &cfg.setup_id, &cfg.setup_id, src)
}

/// What the node does with a state echo for its setup.
#[derive(Debug, PartialEq, Eq)]
enum EchoAction {
    /// Seed the universe from another publisher's buffer.
    Seed(Vec<u8>),
    /// The universe stands: the stored echo is the node's own from an earlier
    /// connection of this process, so it holds nothing newer.
    Keep,
    /// Nothing to do: unreadable, an unknown version, not a full buffer, the
    /// node's own live echo, or a stored copy older than what this connection
    /// has already taken in.
    Ignore,
}

/// Where the connection stands when a state echo for the node's setup arrives.
#[derive(Debug, Clone, Copy)]
struct EchoContext {
    /// The MQTT retain flag as delivered: set only on the stored copy, sent
    /// because this connection subscribed.
    retained: bool,
    /// This connection has taken in the setup's state already.
    settled: bool,
    /// The universe stands on known ground ([`Rig::restored`]).
    restored: bool,
}

/// Decide what a state echo means for the node. There is no clock on the
/// wire, so a stored copy is judged by who stored it and when it arrives.
/// The node's own is never newer than its universe. Once a connection has
/// settled on known ground, any stored copy is older, since the broker
/// doesn't order it against live traffic. Someone else's, taken in on
/// connect, is a change made while the node was away — after an internet
/// outage, typically a device that kept driving the rig over the LAN — and
/// wins over whatever the node had not yet announced.
fn read_echo(payload: &[u8], own_src: &str, ctx: EchoContext) -> EchoAction {
    let Ok(frame) = serde_json::from_slice::<lux_wire::ctl::Frame>(payload) else {
        return EchoAction::Ignore;
    };
    if frame.version() != lux_wire::ctl::VERSION {
        return EchoAction::Ignore;
    }
    let own = frame.src() == Some(own_src);
    let lux_wire::ctl::Frame::Buffer { buffer, .. } = frame else {
        return EchoAction::Ignore;
    };
    if ctx.retained && ctx.settled && ctx.restored {
        return EchoAction::Ignore;
    }
    match (own, ctx.retained) {
        (true, true) => EchoAction::Keep,
        (true, false) => EchoAction::Ignore,
        (false, _) => EchoAction::Seed(buffer),
    }
}

/// Echo the universe if a frame changed it: retained when the per-topic quota
/// allows, live otherwise. Once the quota allows again, a stored copy left
/// behind is caught up.
fn publish_due_echo(client: &AsyncClient, topic: &str, rig: &mut Rig) {
    let now = Instant::now();
    let retain = retain_wait(rig.last_retained, now).is_zero();
    let owed = rig.echo_dirty || (rig.retained_behind && retain);
    if !owed {
        return;
    }
    if !publish_state(client, topic, &rig.universe, &rig.src, retain) {
        return; // stays owed; the next tick retries
    }
    // What the node announces is the ground it stands on from here.
    rig.restored = true;
    rig.echo_dirty = false;
    if retain {
        rig.last_retained = Some(now);
        rig.retained_behind = false;
    } else {
        rig.retained_behind = true;
    }
}

/// Queue the retained presence card. Like [`publish_state`], it never waits on
/// the client's request channel, which the calling `select!` is what drains.
fn publish_presence(client: &AsyncClient, topic: &str, cfg: &NodeConfig, session: &str) {
    let name = format!(
        "lux-node ({})",
        gethostname::gethostname().to_string_lossy()
    );
    let card = lux_wire::ctl::PresenceCard::new(session.to_owned(), cfg.setup_id.clone(), name);
    let Ok(payload) = serde_json::to_vec(&card) else {
        return;
    };
    if let Err(e) = client.try_publish(topic.to_owned(), QoS::AtMostOnce, true, payload) {
        log::debug!("presence publish failed: {e}");
    }
}

/// Queue a state echo without waiting on the client's request channel: the
/// caller runs inside the event loop's `select!`, which is what drains that
/// channel. Returns whether it was queued.
fn publish_state(
    client: &AsyncClient,
    topic: &str,
    universe: &Universe,
    src: &str,
    retain: bool,
) -> bool {
    let frame = lux_wire::ctl::Frame::buffer(universe.slots().to_vec()).with_src(src);
    let Ok(payload) = serde_json::to_vec(&frame) else {
        return false;
    };
    match client.try_publish(topic.to_owned(), QoS::AtMostOnce, retain, payload) {
        Ok(()) => true,
        Err(e) => {
            log::debug!("state echo publish failed: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lux_wire::ctl::Frame;

    fn echo(frame: Frame) -> Vec<u8> {
        serde_json::to_vec(&frame).unwrap()
    }

    #[test]
    fn the_node_names_both_of_its_topics_exactly() {
        let [frame, state] = subscriptions("abc-123", "s-1");
        assert_eq!(frame, lux_wire::ctl::frame_topic("abc-123", "s-1"));
        assert_eq!(state, lux_wire::ctl::state_topic("abc-123", "s-1"));
        // A wildcard here would still carry live traffic, but the broker
        // would never deliver the retained echo a restart seeds from.
        for topic in [&frame, &state] {
            assert!(!topic.contains(['#', '+']), "wildcard in {topic}");
        }
    }

    /// A stored echo arriving on a fresh node's first connection.
    const COLD: EchoContext = EchoContext {
        retained: true,
        settled: false,
        restored: false,
    };

    /// A live echo on a connection that has settled on known ground.
    const RUNNING: EchoContext = EchoContext {
        retained: false,
        settled: true,
        restored: true,
    };

    #[test]
    fn seeds_from_other_publishers_live_or_stored() {
        let theirs = echo(Frame::buffer(vec![1, 2, 3]).with_src("app0"));
        assert_eq!(
            read_echo(&theirs, "node", COLD),
            EchoAction::Seed(vec![1, 2, 3])
        );
        // Once running, live echoes still seed (that's how scene recalls
        // arrive); unstamped (CLI-published) ones too.
        assert_eq!(
            read_echo(&theirs, "node", RUNNING),
            EchoAction::Seed(vec![1, 2, 3])
        );
        assert_eq!(
            read_echo(&echo(Frame::buffer(vec![9])), "node", RUNNING),
            EchoAction::Seed(vec![9])
        );
    }

    #[test]
    fn its_own_stored_echo_keeps_the_universe_and_its_own_live_echo_is_ignored() {
        let own = echo(Frame::buffer(vec![1]).with_src("node"));
        let reconnect = EchoContext {
            restored: true,
            ..COLD
        };
        assert_eq!(read_echo(&own, "node", reconnect), EchoAction::Keep);
        assert_eq!(read_echo(&own, "node", RUNNING), EchoAction::Ignore);
    }

    #[test]
    fn a_stored_copy_after_settling_on_known_ground_is_older_than_what_it_holds() {
        let late = EchoContext {
            retained: true,
            ..RUNNING
        };
        let theirs = echo(Frame::buffer(vec![1]).with_src("app0"));
        assert_eq!(read_echo(&theirs, "node", late), EchoAction::Ignore);
        let own = echo(Frame::buffer(vec![1]).with_src("node"));
        assert_eq!(read_echo(&own, "node", late), EchoAction::Ignore);

        // A fresh node that settled with nothing to stand on still takes a
        // stored copy that turns up late.
        let cold_and_late = EchoContext {
            settled: true,
            ..COLD
        };
        assert_eq!(
            read_echo(&theirs, "node", cold_and_late),
            EchoAction::Seed(vec![1])
        );
    }

    #[test]
    fn someone_elses_stored_state_on_reconnect_is_taken_in() {
        // A peer changed the rig while the node was away — after an internet
        // outage, a device that kept driving it over the LAN. That outranks
        // anything the node hadn't announced from before.
        let theirs = echo(Frame::buffer(vec![1]).with_src("app0"));
        let reconnect = EchoContext {
            restored: true,
            ..COLD
        };
        assert_eq!(
            read_echo(&theirs, "node", reconnect),
            EchoAction::Seed(vec![1])
        );
    }

    #[test]
    fn only_full_buffers_at_a_known_version_count() {
        assert_eq!(
            read_echo(&echo(Frame::channel(1, 255)), "node", COLD),
            EchoAction::Ignore
        );
        assert_eq!(
            read_echo(br#"{"v":9,"buffer":[1]}"#, "node", COLD),
            EchoAction::Ignore
        );
        assert_eq!(read_echo(b"", "node", COLD), EchoAction::Ignore);
    }

    #[test]
    fn frames_held_while_settling_replay_on_top_of_the_seed() {
        let mut rig = Rig {
            universe: Universe::default(),
            lit: false,
            restored: false,
            echo_dirty: false,
            retained_behind: false,
            last_retained: None,
            src: "node".into(),
            send_failures: Cell::new(None),
        };
        let held = vec![RemoteApply::Channel { ch: 2, val: 200 }];
        rig.universe.overlay(&[10, 20, 30]); // the seed
        assert!(rig.replay(held));
        assert_eq!(&rig.universe.slots()[..3], &[10, 200, 30]);

        // A scene recall held back changes nothing on a node.
        assert!(!rig.replay(vec![RemoteApply::Scene { id: "sc".into() }]));
    }

    #[test]
    fn a_failing_send_is_reported_once_then_once_a_minute_then_on_recovery() {
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);

        let (mut run, report) = track_send(None, true, at(0));
        assert_eq!(report, SendReport::Failed);
        // The keepalive retries every second; those retries stay quiet...
        for secs in 1..60 {
            let (next, report) = track_send(run, true, at(secs));
            assert_eq!(report, SendReport::Quiet);
            run = next;
        }
        // ...until a minute has passed, which reports how many went unsaid.
        let (run, report) = track_send(run, true, at(60));
        assert_eq!(report, SendReport::StillFailing(60));
        let (run, report) = track_send(run, true, at(61));
        assert_eq!(report, SendReport::Quiet);
        // Recovery reports the whole run and ends it.
        let (run, report) = track_send(run, false, at(62));
        assert_eq!(report, SendReport::Recovered(62));
        assert_eq!(run, None);
        assert_eq!(track_send(None, false, at(63)), (None, SendReport::Quiet));
    }
}
