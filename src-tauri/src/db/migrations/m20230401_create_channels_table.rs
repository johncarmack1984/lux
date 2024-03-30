use tauri_plugin_sql::{Migration, MigrationKind};

pub fn up() -> Migration {
    Migration {
        version: 1,
        description: "create_channels_table",
        sql: r#"
            CREATE TABLE IF NOT EXISTS channels (
                id SERIAL PRIMARY KEY,
                disabled BOOLEAN NOT NULL,
                channel_number INTEGER NOT NULL UNIQUE,
                label VARCHAR(255) NOT NULL,
                label_color VARCHAR(255) NOT NULL
            );
        "#,
        kind: MigrationKind::Up,
    }
}

pub fn down() -> Migration {
    Migration {
        version: 1,
        description: "drop_channels_table",
        sql: r#"
            DROP TABLE IF EXISTS channels;
        "#,
        kind: MigrationKind::Down,
    }
}
