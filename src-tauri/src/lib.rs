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

use specta_typescript::Typescript;

use buffer::LuxBuffer;
use channels::LuxChannels;
use cmd::*;
use sync::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let builder = setup(tauri::Builder::default(), |_| {});

    let default_buffer = LuxBuffer::from([121, 255, 255, 0, 0, 42]);
    let default_channels = LuxChannels::default();

    let formatter = specta_typescript::formatter::prettier;
    let bigint = specta_typescript::BigIntExportBehavior::Number;
    let bindings = Typescript::default().formatter(formatter).bigint(bigint);
    let router = taurpc::Router::new()
        .export_config(bindings)
        .merge(SyncEndpoint.into_handler())
        .merge(CmdEndpoint.into_handler());

    let taurpc = router.into_handler();

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(logger::logger().build())
        .manage(default_buffer)
        .manage(default_channels)
        .invoke_handler(taurpc)
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
