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
mod remote;
mod sync;

use buffer::{EmitMode, LuxBuffer};
use channels::LuxChannels;
use cmd::*;
use sync::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    let builder = setup(tauri::Builder::default(), |app| {
        remote::connect(app.handle());
        if let Err(e) = build_tray(app) {
            log::error!("tray setup failed: {e}");
        }
    });
    let default_buffer = LuxBuffer::from([121, 255, 255, 0, 0, 42]);
    let default_channels = LuxChannels::default();
    let router = SyncEndpoint
        .into_procedures()
        .merge(CmdEndpoint.into_procedures());
    let taurpc = ttipc::handler(router);
    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(logger::logger().build())
        .manage(default_buffer)
        .manage(default_channels)
        .manage(EmitMode::default())
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

/// Build the menu-bar (tray) menu. Its one toggle flips `EmitMode` so you can
/// feel, on real hardware, whether the UI feels more "real-time" updating before
/// or after the DMX render.
fn build_tray<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    use tauri::image::Image;
    use tauri::menu::{CheckMenuItemBuilder, Menu, MenuItemBuilder, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;
    use tauri::Manager;

    let optimistic = CheckMenuItemBuilder::with_id("toggle_optimistic", "Optimistic light updates")
        .checked(app.state::<EmitMode>().optimistic())
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit lux").build(app)?;
    let menu = Menu::with_items(
        app,
        &[&optimistic, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let toggle = optimistic.clone();
    TrayIconBuilder::with_id("lux-tray")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle_optimistic" => {
                let mode = app.state::<EmitMode>();
                let on = !mode.optimistic();
                mode.set(on);
                let _ = toggle.set_checked(on);
                log::info!(
                    "light updates: {}",
                    if on {
                        "optimistic (before DMX render)"
                    } else {
                        "after DMX render"
                    }
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn ttipc_bindings() -> ttipc::Bindings {
    ttipc::Bindings::new()
        .method_case(ttipc::MethodCase::Snake)
        .router("createTauRPCProxy")
        .register::<CmdMethodsProcedures>()
        .register::<SyncMethodsProcedures>()
        .register_events::<CmdEvent>()
        .register_events::<SyncEvent>()
}
