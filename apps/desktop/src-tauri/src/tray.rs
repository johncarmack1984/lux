//! System-tray menu.
//!
//! Lists the auto-detected DMX devices (USB + network nodes), a "Rescan" action,
//! and quit. The device list is dynamic: it's (re)built from the last
//! [`DmxOutput`] detection result, with the active device checked. Selecting a
//! device switches the live output; "Rescan" re-detects. The optimistic-update
//! toggle is gone — optimistic emits are now always on (a network node in the
//! path makes leading the render mandatory).

use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, IsMenuItem, Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, Runtime};

use crate::devices::{self, DmxOutput};

const TRAY_ID: &str = "lux-tray";

/// Create the tray icon and its (initially empty) menu, wiring the menu-event
/// handler. Devices populate once `devices::rescan` finishes detection.
pub fn build<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let menu = build_menu(app.handle())?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            // Feed raw tray events to the positioner plugin so its Tray*
            // positions know the icon's rect (it caches the last event).
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
        })
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "rescan" => devices::rescan(app),
            "header" | "none" => {}
            key => {
                if let Some(device) = app
                    .state::<DmxOutput>()
                    .devices()
                    .into_iter()
                    .find(|d| d.key() == key)
                {
                    devices::switch_to_device(app, &device);
                    if let Err(e) = refresh(app) {
                        log::warn!("tray refresh failed: {e}");
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuild the menu from the current detection result + active device and set it
/// on the tray. Must run on the main thread (menu mutation is a UI operation).
pub fn refresh<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let output = app.state::<DmxOutput>();
    let devices = output.devices();
    let active = output.active_key();

    let mut items: Vec<Box<dyn IsMenuItem<R>>> = Vec::new();
    items.push(Box::new(
        MenuItemBuilder::with_id("header", "DMX output")
            .enabled(false)
            .build(app)?,
    ));
    if devices.is_empty() {
        items.push(Box::new(
            MenuItemBuilder::with_id("none", "No devices detected")
                .enabled(false)
                .build(app)?,
        ));
    } else {
        for d in &devices {
            items.push(Box::new(
                CheckMenuItemBuilder::with_id(d.key(), &d.label)
                    .checked(d.key() == active)
                    .build(app)?,
            ));
        }
    }
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(
        MenuItemBuilder::with_id("rescan", "Rescan devices").build(app)?,
    ));
    items.push(Box::new(
        MenuItemBuilder::with_id("quit", "Quit lux").build(app)?,
    ));

    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|i| i.as_ref()).collect();
    Menu::with_items(app, &refs)
}
