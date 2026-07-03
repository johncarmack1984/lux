use crate::{devices::DmxOutput, sync::SyncEvent};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
    /// mid-drag when a network node is in the path. Persistence is scheduled
    /// before the render too: the state changed even if no output accepts it,
    /// and the user expects the levels they set to survive a restart either way.
    fn commit<R: Runtime>(
        &self,
        snapshot: Buffer,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        emit_buffer(snapshot.clone(), &app)?;
        schedule_persist(&app);
        render(&snapshot, &app)?;
        Ok(self.clone())
    }
}

// --- persistence (app_config_dir/buffer.json) --------------------------------

/// Restore the last-rendered universe from disk, so a restart brings the light
/// back to how the user left it instead of the seed default. `None` on first
/// run or unreadable data (the caller keeps its default; a corrupt file is
/// logged, not fatal — persistence never sits in the live DMX path).
pub fn restore<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<Buffer> {
    restore_from(&app.path().app_config_dir().ok()?.join("buffer.json"))
}

fn restore_from(path: &Path) -> Option<Buffer> {
    let json = std::fs::read_to_string(path).ok()?;
    match parse_buffer(&json) {
        Ok(buffer) => Some(buffer),
        Err(e) => {
            log::warn!("buffer.json unreadable ({e}); starting from the default buffer");
            None
        }
    }
}

/// Parse a persisted buffer and normalize it to exactly [`UNIVERSE_SIZE`] slots,
/// so a file from an older (shorter-buffer) version still restores cleanly.
fn parse_buffer(json: &str) -> Result<Buffer, serde_json::Error> {
    serde_json::from_str::<Vec<u8>>(json).map(|values| normalize(&values))
}

/// Debounced write of the live buffer. A slider drag calls `set` dozens of
/// times a second, so writes coalesce: the first change queues one writer,
/// which sleeps briefly and then snapshots whatever the buffer holds by the
/// time it wakes — later changes inside the window ride along for free.
/// Best-effort, like the setups store.
fn schedule_persist<R: Runtime>(app: &tauri::AppHandle<R>) {
    static PENDING: AtomicBool = AtomicBool::new(false);
    if PENDING.swap(true, Ordering::SeqCst) {
        return; // a writer is already queued and will pick this change up
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        PENDING.store(false, Ordering::SeqCst);
        let snapshot: Buffer = app.state::<LuxBuffer>().buffer.lock().unwrap().clone();
        save(&app, &snapshot);
    });
}

fn save<R: Runtime>(app: &tauri::AppHandle<R>, buffer: &Buffer) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    match serde_json::to_string(buffer) {
        Ok(json) => {
            if let Err(e) = std::fs::write(dir.join("buffer.json"), json) {
                log::warn!("could not persist the buffer: {e}");
            }
        }
        Err(e) => log::warn!("could not serialize the buffer: {e}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_buffer_normalizes_short_and_long_inputs() {
        // A pre-universe 6-slot file still restores, padded to the full universe.
        let short = parse_buffer("[121,255,255,0,0,42]").unwrap();
        assert_eq!(short.len(), UNIVERSE_SIZE);
        assert_eq!(&short[..6], &[121, 255, 255, 0, 0, 42]);
        assert!(short[6..].iter().all(|&v| v == 0));

        // An oversized file truncates rather than panicking downstream.
        let long = parse_buffer(&serde_json::to_string(&vec![7u8; 600]).unwrap()).unwrap();
        assert_eq!(long.len(), UNIVERSE_SIZE);
        assert!(long.iter().all(|&v| v == 7));
    }

    #[test]
    fn parse_buffer_rejects_garbage() {
        assert!(parse_buffer("not json").is_err());
        assert!(parse_buffer(r#"{"buffer":[1]}"#).is_err());
        // Out-of-range slot values are a type error (u8), not a silent clamp.
        assert!(parse_buffer("[300]").is_err());
    }

    #[test]
    fn restore_from_missing_file_is_none() {
        assert!(restore_from(Path::new("/nonexistent/buffer.json")).is_none());
    }
}
