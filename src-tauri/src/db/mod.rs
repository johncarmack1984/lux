use tauri::path::BaseDirectory;

mod cmd;
mod migrations;

// pub const DB_URL: &str = "libsql://lux-johncarmack1984.turso.io";
pub const DB_URL: &str = "sqlite:mydatabase.db";

pub fn builder() -> tauri_plugin_sql::Builder {
    log::debug!["Building database"];
    log::debug!(
        "tauri::api::path::BaseDirectory::App {:?}",
        BaseDirectory::AppLocalData
    );
    log::info!("DB_URL: {}", DB_URL);
    tauri_plugin_sql::Builder::default().add_migrations(DB_URL, migrations::up())
}
