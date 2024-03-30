use tauri::{tray::TrayIconBuilder, App};

pub fn setup(app: &App) {
    TrayIconBuilder::new()
        .on_tray_icon_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app.app_handle(), &event);
        })
        .build(app)
        .unwrap();
}
