use tauri_plugin_sql::Migration;

mod m20230401_create_channels_table;
mod m20230402_create_buffer_table;

pub fn load() -> Vec<(Migration, Migration)> {
    vec![
        (
            m20230401_create_channels_table::up(),
            m20230401_create_channels_table::down(),
        ),
        (
            m20230402_create_buffer_table::up(),
            m20230402_create_buffer_table::down(),
        ),
    ]
}

pub fn up() -> Vec<Migration> {
    load().into_iter().map(|(up, _)| up).collect()
}

pub fn _down() -> Vec<Migration> {
    load().into_iter().map(|(_, down)| down).collect()
}
