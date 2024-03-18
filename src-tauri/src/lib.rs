use serde::Serialize;
use tauri_plugin_log::LogTarget;

pub const BUFFER_SIZE: usize = 6;

#[derive(Serialize, Clone)]
pub struct LuxState {
    pub buffer: [u8; BUFFER_SIZE],
}

impl Default for LuxState {
    fn default() -> Self {
        Self {
            buffer: [0; BUFFER_SIZE],
        }
    }
}

pub fn logger() -> tauri_plugin_log::Builder {
    tauri_plugin_log::Builder::default().targets([
        LogTarget::LogDir,
        LogTarget::Stdout,
        LogTarget::Webview,
    ])
}
