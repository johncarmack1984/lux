// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod buttons;
mod fixture;
mod interface;
mod tray;

use tauri::Manager;

use crate::buttons::{blackout, full_bright, rgb_chase};
use crate::fixture::slider;
use crate::interface::get_lux_buffer;
use crate::tray::{system_tray, tray_event_handler};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let mut interface = lux::LuxDMX::new().unwrap();
            app.emit_all("lux-state", interface.get_buffer().to_vec())?;
            Ok(())
        })
        .system_tray(system_tray())
        .on_system_tray_event(tray_event_handler)
        .invoke_handler(tauri::generate_handler![
            blackout,
            get_lux_buffer,
            full_bright,
            rgb_chase,
            slider
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
