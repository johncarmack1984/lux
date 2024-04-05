// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod buffer;
mod channel;
mod channels;
mod cmd;
mod colors;
mod devices;
mod error;
mod http;
mod logger;
mod sync;

#[allow(unused_imports)]
use buffer::LuxBuffer;
use channels::LuxChannels;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // #[cfg(debug_assertions)] // only enable instrumentation in development builds
    // let devtools = tauri_plugin_devtools::init();
    let builder = tauri::Builder::default();

    // #[cfg(debug_assertions)]
    // let builder = builder.plugin(devtools);

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(logger::logger().build())
        .manage(LuxBuffer::default())
        .manage(LuxChannels::default())
        .setup(|app| {
            crate::http::setup_http(app)?;
            Ok(())
        })
        // .plugin(tauri_plugin_notification::init())
        // .plugin(tauri_plugin_cli::init())
        .plugin(tauri_plugin_http::init())
        .invoke_handler(tauri::generate_handler![
            cmd::update_channel_value,
            cmd::set_buffer,
            cmd::sync_state,
            cmd::update_channel_metadata,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
