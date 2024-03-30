mod buffer;
mod channel;
mod cmd;
mod colors;
mod db;
mod devices;
mod logger;
mod state;
mod sync;

use crate::state::LuxState;

use tauri::{App, AppHandle, RunEvent};

pub type SetupHook = Box<dyn FnOnce(&mut App) -> Result<(), Box<dyn std::error::Error>> + Send>;
pub type OnEvent = Box<dyn FnMut(&AppHandle, RunEvent)>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(logger::logger().build())
        .plugin(tauri_plugin_cli::init())
        .plugin(db::builder().build())
        .invoke_handler(tauri::generate_handler![
            cmd::update_channel_value,
            cmd::set_buffer,
            cmd::sync_state
        ])
        .manage(LuxState::default().mutex())
        .build(tauri::tauri_build_context!())
        .expect("error while building tauri application")
        .run(move |_app_handle, _event| ())
}
