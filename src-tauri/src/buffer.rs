use crate::{devices::DmxOutput, sync::SyncEvent};
use serde::{Deserialize, Serialize};
use specta::Type;
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

        // Optimistic: reflect the command in the UI immediately, before the DMX
        // render — which may hit the network (sACN) or have no device attached.
        // Always on: with a network node in the path, leading the render is what
        // keeps the sliders from snapping back to a lagging value mid-drag.
        emit_buffer(incoming_buffer, &app)?;
        render(incoming_buffer, &app)?;

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

/// Push the 6-channel buffer to the active DMX output (Enttec USB or sACN
/// network node), selected at startup into `DmxOutput`. Channels map to slots
/// 1..=6 of the 512-channel universe. Fails (e.g. `DEVICE_NOT_FOUND`, socket
/// error) when the output can't accept the frame.
fn render<R: Runtime>(buffer: Buffer, app: &tauri::AppHandle<R>) -> Result<(), String> {
    app.state::<DmxOutput>().render(&buffer)
}

fn emit_buffer<R: Runtime>(buffer: Buffer, app: &tauri::AppHandle<R>) -> Result<(), String> {
    SyncEvent::BufferSet { buffer }
        .emit(app)
        .map_err(|e| format!("Failed to emit buffer_set event: {}", e))
}
