use crate::devices::enttec_open_dmx_usb::EnttecOpenDMX;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Manager, Runtime};

pub const BUFFER_SIZE: usize = 6;

pub trait ConvertFromVec {
    fn convert_from_vec(value: Vec<u8>) -> Self;
}

pub type Buffer = [u8; BUFFER_SIZE];

#[derive(Debug, Serialize, Deserialize, Clone)]
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

// impl From<&mut [u8; BUFFER_SIZE]> for LuxBuffer {
//     fn from(value: &mut [u8; BUFFER_SIZE]) -> Self {
//         LuxBuffer {
//             buffer: Arc::new(Mutex::new(*value)),
//         }
//     }
// }

impl From<Vec<u8>> for LuxBuffer {
    fn from(value: Vec<u8>) -> Self {
        let mut buffer = [0; BUFFER_SIZE];
        buffer[..value.len()].copy_from_slice(&value);
        LuxBuffer::from(buffer)
    }
}

impl Default for LuxBuffer {
    fn default() -> Self {
        LuxBuffer::from([121, 255, 255, 0, 0, 42])
    }
}

impl LuxBuffer {
    pub fn get(&self) -> LuxBuffer {
        let buffer = self.buffer.lock().unwrap();
        LuxBuffer::from(*buffer)
    }

    pub fn set<R: Runtime>(
        &mut self,
        incoming_buffer: Buffer,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxBuffer, String> {
        let mut locked_buffer = self.buffer.lock().unwrap();
        *locked_buffer = incoming_buffer;

        let mut temp_buffer = [0; 513];

        temp_buffer[1..BUFFER_SIZE + 1].copy_from_slice(&incoming_buffer);

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

        app.emit("buffer_set", incoming_buffer)
            .map_err(|e| format!("Failed to emit buffer_set event: {}", e))?;

        Ok(self.clone())
    }

    pub fn set_channel(
        &mut self,
        channel_number: usize,
        value: u8,
        app: tauri::AppHandle,
    ) -> Result<LuxBuffer, String> {
        let mut buffer = *self.buffer.lock().unwrap();
        buffer[channel_number - 1] = value;
        self.set(buffer, app)
    }
}
