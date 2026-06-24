use crate::{devices::enttec_open_dmx_usb::EnttecOpenDMX, sync::SyncEvent};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Manager, Runtime};

pub const BUFFER_SIZE: usize = 6;

#[allow(dead_code)]
pub trait ConvertFromVec {
    fn convert_from_vec(value: Vec<u8>) -> Self;
}

pub type Buffer = [u8; BUFFER_SIZE];

#[derive(Debug, Serialize, Deserialize, Clone, Type)]
pub struct LuxBuffer {
    pub buffer: Arc<Mutex<Buffer>>,
}

/// Tray-toggleable emit ordering for `LuxBuffer::set`. When `optimistic` is set,
/// the buffer event is emitted *before* the DMX render (the UI reflects a command
/// immediately, even with no fixture attached); otherwise it is emitted only after
/// a successful render (the UI mirrors the hardware). Default: optimistic, so the
/// UI reflects a command immediately instead of waiting on the slower DMX render
/// (waiting makes the sliders snap back to the lagging value and judder mid-drag).
pub struct EmitMode {
    optimistic: AtomicBool,
}

impl Default for EmitMode {
    fn default() -> Self {
        EmitMode {
            optimistic: AtomicBool::new(true),
        }
    }
}

impl EmitMode {
    pub fn optimistic(&self) -> bool {
        self.optimistic.load(Ordering::Relaxed)
    }

    pub fn set(&self, optimistic: bool) {
        self.optimistic.store(optimistic, Ordering::Relaxed);
    }
}

impl ConvertFromVec for Buffer {
    fn convert_from_vec(value: Vec<u8>) -> Self {
        let mut buffer = [0; BUFFER_SIZE];
        buffer[..value.len()].copy_from_slice(&value);
        buffer
    }
}

impl From<LuxBuffer> for Buffer {
    fn from(value: LuxBuffer) -> Self {
        value.buffer.lock().as_deref().unwrap().clone()
    }
}

impl From<&LuxBuffer> for Buffer {
    fn from(value: &LuxBuffer) -> Self {
        *value.buffer.lock().unwrap()
    }
}

impl From<Buffer> for LuxBuffer {
    fn from(value: Buffer) -> Self {
        LuxBuffer {
            buffer: Arc::new(Mutex::new(value)),
        }
    }
}

impl LuxBuffer {
    pub fn set<R: Runtime>(
        &mut self,
        incoming_buffer: Buffer,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        *self.buffer.lock().unwrap() = incoming_buffer;

        // Tray toggle: emit the buffer event before the DMX render (optimistic —
        // the UI reflects the command even with no fixture) or only after a
        // successful render (the UI mirrors the hardware).
        let optimistic = app.state::<EmitMode>().optimistic();
        if optimistic {
            emit_buffer(incoming_buffer, &app)?;
        }
        render(incoming_buffer)?;
        if !optimistic {
            emit_buffer(incoming_buffer, &app)?;
        }

        Ok(self.clone())
    }

    pub fn set_channel<R: Runtime>(
        &mut self,
        channel_number: usize,
        value: u8,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        let mut buffer = *self.buffer.lock().unwrap();
        buffer[channel_number - 1] = value;
        self.set(buffer, app)
    }
}

/// Push the 6-channel buffer to the Enttec OpenDMX fixture (channels 1..=6 of the
/// 512-channel universe). Fails (e.g. `DEVICE_NOT_FOUND`) when no unit is attached.
fn render(buffer: Buffer) -> Result<(), String> {
    let mut temp_buffer = [0u8; 513];
    temp_buffer[1..BUFFER_SIZE + 1].copy_from_slice(&buffer);

    let mut interface = EnttecOpenDMX::new()
        .map_err(|e| format!("Enttec OpenDMX USB initialization failed: {}", e))?;
    interface
        .open()
        .map_err(|e| format!("Enttec OpenDMX USB failed to open: {}", e))?;
    interface.set_buffer(temp_buffer);
    interface
        .render()
        .map_err(|e| format!("Enttec OpenDMX USB failed to render: {}", e))?;
    interface
        .close()
        .map_err(|e| format!("Enttec OpenDMX USB failed to close: {}", e))?;
    Ok(())
}

fn emit_buffer<R: Runtime>(buffer: Buffer, app: &tauri::AppHandle<R>) -> Result<(), String> {
    SyncEvent::BufferSet { buffer }
        .emit(app)
        .map_err(|e| format!("Failed to emit buffer_set event: {}", e))
}
