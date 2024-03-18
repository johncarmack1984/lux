// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod tray;

use crate::commands::{blackout, full_bright, update_channel_value};
use crate::tray::{system_tray, tray_event_handler};
use lux::{logger, lux_state, LuxState};

fn main() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .manage(lux_state())
        .plugin(logger().build())
        .system_tray(system_tray())
        .on_system_tray_event(tray_event_handler)
        .invoke_handler(tauri::generate_handler![
            blackout,
            full_bright,
            update_channel_value
        ])
        .run(tauri::generate_context!())
}
