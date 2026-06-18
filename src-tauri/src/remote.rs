//! Remote control over AWS IoT Core (MQTT).
//!
//! lux dials *out* to IoT Core (mutual TLS) and subscribes to a control
//! topic; the `lux-discord-bot` Lambda publishes buffer commands there.
//! This replaces the old ngrok HTTP tunnel: no public ingress, nothing
//! always-on, one AWS account. Applying a command runs the same
//! `LuxBuffer::set`, which emits `SyncEvent::BufferSet`, so the desktop UI
//! reacts to a remote command exactly as it does to a local one.
//!
//! Everything is read from the environment (see `src-tauri/.env.example`).
//! If the `AWS_IOT_*` vars are absent, remote control is skipped and the
//! rest of the app runs normally.

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
    let _ = dotenvy::dotenv();
    let endpoint = std::env::var("AWS_IOT_ENDPOINT").ok()?;
    let device_id = std::env::var("LUX_DEVICE_ID").unwrap_or_else(|_| "lux-1".into());
    let ca = std::fs::read(std::env::var("AWS_IOT_ROOT_CA_PATH").ok()?).ok()?;
    let cert = std::fs::read(std::env::var("AWS_IOT_CERT_PATH").ok()?).ok()?;
    let key = std::fs::read(std::env::var("AWS_IOT_KEY_PATH").ok()?).ok()?;
    Some(Config {
        topic: format!("lux/{device_id}/buffer/set"),
        client_id: device_id,
        endpoint,
        ca,
        cert,
        key,
    })
}

/// Spawn the IoT Core listener if configured; otherwise log and no-op.
pub fn connect<R: Runtime>(app: &AppHandle<R>) {
    let Some(cfg) = load_config() else {
        log::info!(
            "AWS IoT not configured; remote control disabled (set AWS_IOT_* in .env to enable)"
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
