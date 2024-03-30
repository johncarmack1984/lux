mod cmd;
mod migrations;

// pub const DB_URL: &str = "libsql://lux-johncarmack1984.turso.io";
pub const DB_URL: &str = "sqlite:mydatabase.db";

pub fn builder() -> tauri_plugin_sql::Builder {
    tauri_plugin_sql::Builder::default().add_migrations(DB_URL, migrations::up())
}
