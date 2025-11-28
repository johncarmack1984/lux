// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod buffer;
mod channel;
mod channels;
mod cmd;
mod colors;
mod devices;
mod error;
mod logger;
mod sync;

use buffer::LuxBuffer;
use channels::LuxChannels;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let builder = setup(tauri::Builder::default(), |_| {});

    let default_buffer = LuxBuffer::from([121, 255, 255, 0, 0, 42]);
    let default_channels = LuxChannels::default();

    let formatter = specta_typescript::formatter::prettier;
    let bindings = specta_typescript::Typescript::default().formatter(formatter);
    let router = taurpc::Router::new().export_config(bindings).into_handler();

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(logger::logger().build())
        .manage(default_buffer)
        .manage(default_channels)
        .invoke_handler(router)
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}

pub fn setup<R, F>(builder: tauri::Builder<R>, setup: F) -> tauri::Builder<R>
where
    R: tauri::Runtime,
    F: FnOnce(&tauri::App<R>) + Send + 'static,
{
    builder.setup(move |app| Ok(setup(app)))
}
