//! Remote control over AWS IoT Core (MQTT).
//!
//! lux dials *out* to IoT Core (mutual TLS) and subscribes to a control
//! topic; the `lux-discord-bot` Lambda publishes buffer commands there.
//! This replaces the old ngrok HTTP tunnel: no public ingress, nothing
//! always-on, one AWS account. Applying a command runs the same
//! `LuxBuffer::set`, which emits `SyncEvent::BufferSet`, so the desktop UI
//! reacts to a remote command exactly as it does to a local one.
//!
//! Configured by the `remoteControl` section of the gitignored
//! `endpoints.local.json` (device identity + mTLS material paths — see
//! [`crate::endpoints`]); this is per-machine provisioning, so the generated
//! prod config never carries it. When the section is absent, remote control is
//! skipped and the rest of the app runs normally.

use crate::buffer::{Buffer, LuxBuffer};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use serde::Deserialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

/// `{ "buffer": [r, g, b, a, w, ...] }` — same body the old HTTP `/buffer` took.
#[derive(Deserialize)]
struct BufferCommand {
    buffer: Buffer,
}

struct Config {
    endpoint: String,
    client_id: String,
    topic: String,
    ca: Vec<u8>,
    cert: Vec<u8>,
    key: Vec<u8>,
}

fn load_config() -> Option<Config> {
    let rc = crate::endpoints::effective().remote_control.clone()?;
    let read = |what: &str, path: &str| {
        std::fs::read(path)
            .inspect_err(|e| log::warn!("remote control {what} unreadable at {path}: {e}"))
            .ok()
    };
    Some(Config {
        topic: format!("lux/{}/buffer/set", rc.device_id),
        client_id: rc.device_id,
        endpoint: rc.endpoint,
        ca: read("root CA", &rc.root_ca_path)?,
        cert: read("device cert", &rc.cert_path)?,
        key: read("device key", &rc.key_path)?,
    })
}

/// Spawn the IoT Core listener if configured; otherwise log and no-op.
pub fn connect<R: Runtime>(app: &AppHandle<R>) {
    let Some(cfg) = load_config() else {
        log::info!(
            "remote control not configured (no remoteControl in endpoints.local.json); disabled"
        );
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(cfg, app).await {
            log::error!("IoT remote-control loop exited: {e}");
        }
    });
}

async fn run<R: Runtime>(cfg: Config, app: AppHandle<R>) -> Result<(), String> {
    let Config {
        endpoint,
        client_id,
        topic,
        ca,
        cert,
        key,
    } = cfg;

    let mut opts = MqttOptions::new(client_id, endpoint, 8883);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_transport(Transport::Tls(TlsConfiguration::Simple {
        ca,
        alpn: None,
        client_auth: Some((cert, key)),
    }));

    let (client, mut eventloop) = AsyncClient::new(opts, 10);
    client
        .subscribe(topic.clone(), QoS::AtLeastOnce)
        .await
        .map_err(|e| format!("subscribe to {topic} failed: {e}"))?;
    log::info!("IoT remote control connected; listening on {topic}");

    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                if let Err(e) = apply(&publish.payload, &app) {
                    log::warn!("ignoring IoT command: {e}");
                }
            }
            Ok(_) => {}
            Err(e) => {
                // rumqttc reconnects on the next poll; back off briefly.
                log::warn!("IoT connection error (retrying): {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

fn apply<R: Runtime>(payload: &[u8], app: &AppHandle<R>) -> Result<(), String> {
    let cmd: BufferCommand =
        serde_json::from_slice(payload).map_err(|e| format!("bad payload: {e}"))?;
    log::debug!("IoT buffer command: {:?}", cmd.buffer);
    let mut state = app.state::<LuxBuffer>().inner().clone();
    state.set(cmd.buffer, app.clone()).map(|_| ())
}
