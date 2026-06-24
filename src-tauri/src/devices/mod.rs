//! DMX output transports + device auto-detection.
//!
//! A [`DmxSink`] is "somewhere to push the fixture's channel bytes". Two impls
//! exist: the Enttec Open DMX USB (a local FTDI device) and sACN/E1.31
//! multicast (network nodes like the DMXKing eDMX1 Pro). [`detect_devices`]
//! finds what's actually present — FTDI over USB and Art-Net nodes over the
//! network — and the tray lets you pick one; the chosen sink lives in
//! [`DmxOutput`] and is swapped in at runtime. The render path
//! (`buffer::render`) stays transport-agnostic.

pub mod discovery;
pub mod enttec_open_dmx_usb;
pub mod sacn;

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

/// A DMX output: take the fixture's channel bytes (lux uses 6: RGBAW + master)
/// and push them to hardware.
pub trait DmxSink: Send + Sync {
    fn render(&self, channels: &[u8]) -> Result<(), String>;
}

/// The kind of transport behind a [`Device`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Enttec,
    Sacn,
}

impl Transport {
    /// Network transports must be re-sent periodically (the node times out and
    /// drops the light otherwise); the USB device holds its last frame.
    pub fn needs_keepalive(self) -> bool {
        matches!(self, Transport::Sacn)
    }
}

/// A detected output the user can select from the tray.
#[derive(Debug, Clone)]
pub struct Device {
    pub transport: Transport,
    /// sACN universe to drive (auto-detected via Art-Net); ignored for Enttec.
    pub universe: u16,
    /// Human-readable menu label, e.g. `eDMX1 PRO — 192.168.1.111 · U1`.
    pub label: String,
}

impl Device {
    /// Stable identity used as the tray menu-item id and persistence key.
    pub fn key(&self) -> String {
        match self.transport {
            Transport::Enttec => "enttec".to_string(),
            Transport::Sacn => format!("sacn:{}", self.universe),
        }
    }
}

/// Detect available outputs: FTDI/USB (Enttec) and Art-Net/sACN network nodes.
/// Network discovery blocks ~1.5s, so call this off the UI thread.
pub fn detect_devices() -> Vec<Device> {
    let mut devices = Vec::new();

    // USB: any FTDI device present means an Enttec-compatible interface is here.
    match libftd2xx::list_devices() {
        Ok(infos) if !infos.is_empty() => {
            let detail = infos
                .iter()
                .map(|i| i.description.clone())
                .find(|d| !d.is_empty())
                .unwrap_or_else(|| "FT232".to_string());
            log::info!("detected USB FTDI interface ({detail})");
            devices.push(Device {
                transport: Transport::Enttec,
                universe: 1,
                label: format!("Enttec Open DMX USB ({detail})"),
            });
        }
        Ok(_) => {}
        Err(e) => log::debug!("FTDI enumeration: {e:?}"),
    }

    // Network: Art-Net discovery — each output-capable node becomes an sACN device.
    // Nodes reply with a random delay, so give discovery a few seconds.
    for n in discovery::discover(Duration::from_millis(3000)) {
        if !n.output {
            continue;
        }
        log::info!(
            "detected Art-Net node {} ({}, fw {}.{}) at {} -> sACN universe {}",
            n.short_name,
            n.long_name,
            n.firmware.0,
            n.firmware.1,
            n.ip,
            n.sacn_universe()
        );
        devices.push(Device {
            transport: Transport::Sacn,
            universe: n.sacn_universe(),
            label: format!("{} — {} · U{}", n.short_name, n.ip, n.sacn_universe()),
        });
    }
    devices
}

/// Pick which detected device to make active: the remembered one if still
/// present, else the lone device, else prefer a network node, else nothing.
fn choose_active(devices: &[Device], saved: Option<&str>) -> Option<Device> {
    if let Some(key) = saved {
        if let Some(d) = devices.iter().find(|d| d.key() == key) {
            return Some(d.clone());
        }
    }
    devices
        .iter()
        .find(|d| d.transport == Transport::Sacn)
        .or_else(|| devices.first())
        .cloned()
}

/// Tauri-managed state: the active output sink plus the last detection result
/// (so the tray can list devices). The sink is swapped in place on switch, so
/// all callers — render path and keepalive — see the new device.
pub struct DmxOutput {
    inner: Mutex<Active>,
    devices: Mutex<Vec<Device>>,
}

struct Active {
    transport: Transport,
    sink: Arc<dyn DmxSink>,
    key: String,
}

impl Default for DmxOutput {
    fn default() -> Self {
        // No device selected until detection runs (right at startup).
        DmxOutput {
            inner: Mutex::new(Active {
                transport: Transport::Enttec,
                sink: Arc::new(NullSink),
                key: String::new(),
            }),
            devices: Mutex::new(Vec::new()),
        }
    }
}

impl DmxOutput {
    pub fn render(&self, channels: &[u8]) -> Result<(), String> {
        // Clone the Arc out of the lock so a (possibly slow) render doesn't hold
        // it — a tray switch can then proceed concurrently.
        let sink = self.inner.lock().unwrap().sink.clone();
        sink.render(channels)
    }

    pub fn needs_keepalive(&self) -> bool {
        self.inner.lock().unwrap().transport.needs_keepalive()
    }

    pub fn active_key(&self) -> String {
        self.inner.lock().unwrap().key.clone()
    }

    pub fn devices(&self) -> Vec<Device> {
        self.devices.lock().unwrap().clone()
    }

    fn set_devices(&self, devices: Vec<Device>) {
        *self.devices.lock().unwrap() = devices;
    }

    fn set_device(&self, device: &Device) {
        let sink = build_sink(device);
        let mut active = self.inner.lock().unwrap();
        active.transport = device.transport;
        active.sink = sink;
        active.key = device.key();
    }
}

/// A sink that drops frames — active when nothing is selected or a transport
/// can't initialize, so the UI still works (optimistic emits) while we log why.
struct NullSink;
impl DmxSink for NullSink {
    fn render(&self, _channels: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

fn build_sink(device: &Device) -> Arc<dyn DmxSink> {
    match device.transport {
        Transport::Enttec => Arc::new(enttec_open_dmx_usb::EnttecSink),
        Transport::Sacn => match sacn::SacnSink::new(device.universe, sacn_interface_override()) {
            Ok(sink) => Arc::new(sink),
            Err(e) => {
                log::error!("sACN init failed ({e}); output disabled until reselected");
                Arc::new(NullSink)
            }
        },
    }
}

/// Optional advanced override for which local NIC sends multicast (multi-homed
/// machines); normally unset — the OS routes out the interface the node is on.
fn sacn_interface_override() -> Option<Ipv4Addr> {
    let _ = dotenvy::dotenv();
    std::env::var("LUX_SACN_INTERFACE")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
}

// --- runtime selection + auto-detect ---------------------------------------

/// Make `device` the active output: build its sink, remember it, and push the
/// current buffer so it lights up immediately. Cheap (no discovery) — safe to
/// call on the UI thread from the tray handler.
pub fn switch_to_device<R: Runtime>(app: &AppHandle<R>, device: &Device) {
    app.state::<DmxOutput>().set_device(device);
    persist_key(app, &device.key());
    let buffer = *app.state::<crate::buffer::LuxBuffer>().buffer.lock().unwrap();
    if let Err(e) = app.state::<DmxOutput>().render(&buffer) {
        log::trace!("post-switch render failed: {e}");
    }
    log::info!("active DMX output: {}", device.label);
}

/// Manual rescan (tray "Rescan"): one detection pass off the UI thread, then
/// auto-select and rebuild the menu.
pub fn rescan<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || apply_detection(&app, detect_devices()));
}

/// Startup auto-detect: retry until a device appears. The first scan right after
/// launch often comes up empty — the network stack (and macOS local-network
/// access) is still warming up, so the eDMX's reply misses that first window.
/// Retry a few times before giving up, so no manual "Rescan" is needed.
pub fn start_autodetect<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || {
        for attempt in 1..=6 {
            let devices = detect_devices();
            if !devices.is_empty() {
                apply_detection(&app, devices);
                return;
            }
            log::debug!("auto-detect attempt {attempt}: nothing yet, retrying");
            std::thread::sleep(Duration::from_secs(1));
        }
        log::info!("auto-detect found nothing; connect a device and use the tray's Rescan");
    });
}

/// Apply a detection result: pick the active device and refresh the tray. Menu
/// mutation must run on the main thread.
fn apply_detection<R: Runtime>(app: &AppHandle<R>, devices: Vec<Device>) {
    let active = choose_active(&devices, saved_key(app).as_deref());
    app.state::<DmxOutput>().set_devices(devices);
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(device) = active {
            switch_to_device(&app, &device);
        }
        if let Err(e) = crate::tray::refresh(&app) {
            log::warn!("tray refresh failed: {e}");
        }
    });
}

// --- persistence ------------------------------------------------------------

fn persist_key<R: Runtime>(app: &AppHandle<R>, key: &str) {
    if let Some(path) = key_file(app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, key) {
            log::warn!("could not persist device choice: {e}");
        }
    }
}

fn saved_key<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    key_file(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn key_file<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("dmx-device"))
}

// --- keepalive --------------------------------------------------------------

/// Spawn the keepalive loop for the life of the app. Each second it re-sends the
/// current buffer *only* while a network transport is active, so the node
/// doesn't time out and drop the light when the UI is idle. Sends directly
/// through the sink, bypassing `LuxBuffer::set`, so it never re-emits UI events.
pub fn start_keepalive<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            if !app.state::<DmxOutput>().needs_keepalive() {
                continue;
            }
            // Copy out of the lock before the render so no guard is held across it.
            let buffer = *app.state::<crate::buffer::LuxBuffer>().buffer.lock().unwrap();
            if let Err(e) = app.state::<DmxOutput>().render(&buffer) {
                log::trace!("sACN keepalive render failed: {e}");
            }
        }
    });
}
