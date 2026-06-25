use crate::{devices::DmxOutput, sync::SyncEvent};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, Mutex};
use tauri::{Manager, Runtime};

/// A full DMX512 universe is 512 one-byte slots. Lux drives the whole universe:
/// the RGBAW fixture lives at slots 1..=6, slots 7..=512 are free for raw control.
pub const UNIVERSE_SIZE: usize = 512;

/// The render buffer: one byte per DMX slot. Carried over IPC as `number[]`
/// (specta emits a plain array, not a 512-long tuple). The stored buffer is
/// always exactly `UNIVERSE_SIZE` bytes, so every consumer can rely on that.
pub type Buffer = Vec<u8>;

#[derive(Debug, Serialize, Deserialize, Clone, Type)]
pub struct LuxBuffer {
    pub buffer: Arc<Mutex<Buffer>>,
}

/// Pad/truncate an incoming slice to exactly `UNIVERSE_SIZE` slots — used when
/// seeding a fresh buffer.
fn normalize(value: &[u8]) -> Buffer {
    let mut buffer = vec![0u8; UNIVERSE_SIZE];
    let n = value.len().min(UNIVERSE_SIZE);
    buffer[..n].copy_from_slice(&value[..n]);
    buffer
}

impl From<LuxBuffer> for Buffer {
    fn from(value: LuxBuffer) -> Self {
        value.buffer.lock().unwrap().clone()
    }
}

impl From<&LuxBuffer> for Buffer {
    fn from(value: &LuxBuffer) -> Self {
        value.buffer.lock().unwrap().clone()
    }
}

impl From<Vec<u8>> for LuxBuffer {
    fn from(value: Vec<u8>) -> Self {
        LuxBuffer {
            buffer: Arc::new(Mutex::new(normalize(&value))),
        }
    }
}

impl LuxBuffer {
    /// Overlay `incoming` onto the universe starting at slot 1, leaving any
    /// higher slots untouched, then emit + render. Overlay (not replace) is what
    /// lets a 6-byte RGBAW write — from the color picker or the Discord remote —
    /// leave the raw channels 7..=512 alone.
    pub fn set<R: Runtime>(
        &mut self,
        incoming: Buffer,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        let snapshot = {
            let mut guard = self.buffer.lock().unwrap();
            let n = incoming.len().min(guard.len());
            guard[..n].copy_from_slice(&incoming[..n]);
            guard.clone()
        };
        self.commit(snapshot, app)
    }

    pub fn set_channel<R: Runtime>(
        &mut self,
        channel_number: usize,
        value: u8,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        if !(1..=UNIVERSE_SIZE).contains(&channel_number) {
            return Err(format!(
                "channel {channel_number} out of range (expected 1..={UNIVERSE_SIZE})"
            ));
        }
        let snapshot = {
            let mut guard = self.buffer.lock().unwrap();
            guard[channel_number - 1] = value;
            guard.clone()
        };
        self.commit(snapshot, app)
    }

    /// Shared tail of `set`/`set_channel`: reflect the new buffer in the UI
    /// optimistically — *before* the render, which may hit the network (sACN) or
    /// have no device attached — then push it to the active DMX output. Leading
    /// the render keeps the sliders from snapping back to a lagging value
    /// mid-drag when a network node is in the path.
    fn commit<R: Runtime>(
        &self,
        snapshot: Buffer,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        emit_buffer(snapshot.clone(), &app)?;
        render(&snapshot, &app)?;
        Ok(self.clone())
    }
}

/// Push the universe buffer to the active DMX output (Enttec USB or sACN network
/// node), selected at startup into `DmxOutput`. Buffer slot N maps to DMX slot N.
/// Fails (e.g. `DEVICE_NOT_FOUND`, socket error) when the output can't accept it.
fn render<R: Runtime>(buffer: &[u8], app: &tauri::AppHandle<R>) -> Result<(), String> {
    app.state::<DmxOutput>().render(buffer)
}

fn emit_buffer<R: Runtime>(buffer: Buffer, app: &tauri::AppHandle<R>) -> Result<(), String> {
    SyncEvent::BufferSet { buffer }
        .emit(app)
        .map_err(|e| format!("Failed to emit buffer_set event: {}", e))
}
