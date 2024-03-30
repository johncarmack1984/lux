use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub const BUFFER_SIZE: usize = 6;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LuxBuffer(Arc<Mutex<[u8; BUFFER_SIZE]>>);

impl LuxBuffer {
    pub fn get(&self) -> [u8; BUFFER_SIZE] {
        let buffer = self.0.lock().unwrap();
        *buffer
    }

    pub fn set(&self, buffer: [u8; BUFFER_SIZE]) {
        let mut locked_buffer = self.0.lock().unwrap();
        *locked_buffer = buffer;
    }
}

impl From<[u8; BUFFER_SIZE]> for LuxBuffer {
    fn from(value: [u8; BUFFER_SIZE]) -> Self {
        LuxBuffer(Arc::new(Mutex::new(value)))
    }
}

impl Default for LuxBuffer {
    fn default() -> Self {
        LuxBuffer::from([121, 255, 255, 0, 0, 42])
    }
}
