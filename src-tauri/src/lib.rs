mod buffer;
mod channel;
mod cmd;
mod colors;
mod db;
mod devices;
mod error;
mod logger;
mod positioner;
mod state;
mod sync;

use crate::state::LuxState;

use tauri::{App, AppHandle, Manager, RunEvent};

pub type SetupHook = Box<dyn FnOnce(&mut App) -> Result<(), Box<dyn std::error::Error>> + Send>;
pub type OnEvent = Box<dyn FnMut(&AppHandle, RunEvent)>;

#[derive(Clone, serde::Serialize)]
struct Payload {
    args: Vec<String>,
    cwd: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_cli::init())
        .plugin(logger::logger().build())
        .plugin(tauri_plugin_positioner::init())
        .setup(|app| Ok(positioner::setup(app)))
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(db::builder().build())
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            println!("{}, {argv:?}, {cwd}", app.package_info().name);
            app.emit("single-instance", Payload { args: argv, cwd })
                .unwrap();
        }))
        .manage(LuxState::default().mutex())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            cmd::update_channel_value,
            cmd::set_buffer,
            cmd::sync_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
