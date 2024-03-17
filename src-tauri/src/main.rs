// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod buttons;
mod fixture;
mod tray;

use crate::buttons::{blackout, full_bright, rgb_chase};
use crate::fixture::slider;
use crate::tray::{system_tray, tray_event_handler};

fn main() {
    tauri::Builder::default()
        .system_tray(system_tray())
        .on_system_tray_event(tray_event_handler)
        .invoke_handler(tauri::generate_handler![
            blackout,
            full_bright,
            rgb_chase,
            slider
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
