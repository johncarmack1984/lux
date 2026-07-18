//! The node's run loop: one user-channel connection, frames applied to a
//! plain universe buffer, sACN out.
//!
//! Mirrors the desktop listener's connection shape (`nudge.rs`) minus Tauri:
//! WSS to IoT Core through the `lux-sync-auth` custom authorizer with a fresh
//! Cognito ID token per attempt, a retained presence card cleared by the Last
//! Will, and capped exponential backoff. Differences fitting a render node:
//! it subscribes only the ctl space (a node never syncs), it seeds its buffer
//! from the retained state echo on connect (restart persistence via the
//! broker), it re-renders every second (sACN receivers drop a source that
//! goes quiet), and it publishes its own retained state echo after applying.

use std::time::Duration;

use lux_engine::ctl::{gate, route, RemoteApply, Route};
use lux_engine::sacn::SacnSink;
use lux_engine::tls::webpki_pem_bundle;
use lux_engine::universe::Universe;
use lux_engine::DmxSink;
use rumqttc::{
    AsyncClient, ConnectionError, Event, LastWill, MqttOptions, Packet, QoS, TlsConfiguration,
    Transport,
};
use uuid::Uuid;

use crate::auth;
use crate::config::{Endpoints, NodeConfig, StoredSession};

/// sACN keepalive: receivers time a source out after ~2.5s of silence.
const KEEPALIVE: Duration = Duration::from_secs(1);
/// Trailing-edge coalescing for the retained state echo (≤5 Hz).
const ECHO_WINDOW: Duration = Duration::from_millis(200);

pub async fn run(
    env: Endpoints,
    cfg: NodeConfig,
    session: StoredSession,
    sink: SacnSink,
) -> Result<(), String> {
    let mut universe = Universe::default();
    let mut refresh_token = session.refresh_token;
    let mut backoff_secs = 1u64;

    loop {
        // Fresh ID token every attempt; Cognito may rotate the refresh token.
        let tokens = match auth::refresh(&env, &refresh_token).await {
            Ok(tokens) => tokens,
            Err(e) => {
                log::warn!("token refresh failed ({e}); retrying in {backoff_secs}s");
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
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

        run_connection(
            &env,
            &cfg,
            &sink,
            &mut universe,
            &sub,
            tokens.id,
            &mut backoff_secs,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}

#[allow(clippy::too_many_arguments)] // one call site; a struct would be noise
async fn run_connection(
    env: &Endpoints,
    cfg: &NodeConfig,
    sink: &SacnSink,
    universe: &mut Universe,
    sub: &str,
    token: String,
    backoff_secs: &mut u64,
) {
    // Random per-session suffix: shares the peers' client-id prefix (the
    // authorizer allows it) and stamps our own frames/echoes.
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
    if let Err(e) = client
        .subscribe(lux_wire::ctl::user_filter(sub), QoS::AtMostOnce)
        .await
    {
        log::warn!("could not queue the ctl subscribe: {e}");
        return;
    }

    let mut keepalive = tokio::time::interval(KEEPALIVE);
    let mut echo_tick = tokio::time::interval(ECHO_WINDOW);
    let mut echo_dirty = false;

    loop {
        tokio::select! {
            _ = keepalive.tick() => {
                // Continuous transmission: receivers hold our look only while
                // we keep sending it.
                if let Err(e) = sink.render(universe.slots()) {
                    log::warn!("sACN render failed: {e}");
                }
            }
            _ = echo_tick.tick() => {
                if echo_dirty {
                    echo_dirty = false;
                    publish_state(&client, &state_topic, universe, &session).await;
                }
            }
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    log::info!("user channel connected; applying setup {}", cfg.setup_id);
                    *backoff_secs = 1;
                    publish_presence(&client, &presence_topic, cfg, &session).await;
                    // Announce our current truth for whoever is watching.
                    publish_state(&client, &state_topic, universe, &session).await;
                }
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    match route(&publish.topic, sub) {
                        Route::Frame { setup_id } if setup_id == cfg.setup_id => {
                            if apply_frame(&publish.payload, cfg, universe, &session) {
                                if let Err(e) = sink.render(universe.slots()) {
                                    log::warn!("sACN render failed: {e}");
                                }
                                echo_dirty = true;
                            }
                        }
                        Route::State { setup_id }
                            if setup_id == cfg.setup_id =>
                        {
                            // Another applier's retained truth (or a surface's
                            // echo): seed/converge without re-echoing it.
                            seed_from_state(&publish.payload, universe, &session, sink);
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
                    return;
                }
            }
        }
    }
}

/// Parse + gate + apply one ctl frame; returns whether the universe changed.
fn apply_frame(payload: &[u8], cfg: &NodeConfig, universe: &mut Universe, session: &str) -> bool {
    let frame: lux_wire::ctl::Frame = match serde_json::from_slice(payload) {
        Ok(frame) => frame,
        Err(e) => {
            log::warn!("ignoring unreadable ctl frame: {e}");
            return false;
        }
    };
    match gate(frame, &cfg.setup_id, &cfg.setup_id, session) {
        Some(RemoteApply::Overlay(bytes)) => {
            universe.overlay(&bytes);
            true
        }
        Some(RemoteApply::Channel { ch, val }) => match universe.set_slot(ch, val) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("ctl frame apply failed: {e}");
                false
            }
        },
        None => false,
    }
}

/// Seed the universe from a retained state echo (someone else's truth).
fn seed_from_state(payload: &[u8], universe: &mut Universe, session: &str, sink: &SacnSink) {
    let Ok(frame) = serde_json::from_slice::<lux_wire::ctl::Frame>(payload) else {
        return;
    };
    if frame.version() != lux_wire::ctl::VERSION || frame.src() == Some(session) {
        return;
    }
    if let lux_wire::ctl::Frame::Buffer { buffer, .. } = frame {
        universe.overlay(&buffer);
        if let Err(e) = sink.render(universe.slots()) {
            log::warn!("sACN render failed: {e}");
        }
        log::info!("seeded the universe from the retained state echo");
    }
}

async fn publish_presence(
    client: &AsyncClient,
    topic: &str,
    cfg: &NodeConfig,
    session: &str,
) -> () {
    let name = format!(
        "lux-node ({})",
        gethostname::gethostname().to_string_lossy()
    );
    let card = lux_wire::ctl::PresenceCard::new(session.to_owned(), cfg.setup_id.clone(), name);
    let Ok(payload) = serde_json::to_vec(&card) else {
        return;
    };
    if let Err(e) = client
        .publish(topic.to_owned(), QoS::AtMostOnce, true, payload)
        .await
    {
        log::debug!("presence publish failed: {e}");
    }
}

async fn publish_state(client: &AsyncClient, topic: &str, universe: &Universe, session: &str) {
    let frame = lux_wire::ctl::Frame::buffer(universe.slots().to_vec()).with_src(session);
    let Ok(payload) = serde_json::to_vec(&frame) else {
        return;
    };
    if let Err(e) = client
        .publish(topic.to_owned(), QoS::AtMostOnce, true, payload)
        .await
    {
        log::debug!("state echo publish failed: {e}");
    }
}
