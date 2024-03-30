use tauri_plugin_sql::{Migration, MigrationKind};

pub fn up() -> Migration {
    Migration {
        version: 2,
        description: "create_buffer_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS buffer (
                id SERIAL PRIMARY KEY,
                data BYTEA NOT NULL
            );
        "#,
        kind: MigrationKind::Up,
    }
}

pub fn down() -> Migration {
    Migration {
        version: 2,
        description: "drop_buffer_table",
        sql: r#"
            DROP TABLE IF EXISTS buffer;
        "#,
        kind: MigrationKind::Down,
    }
}
