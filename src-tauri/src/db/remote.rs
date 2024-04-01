use dotenvy::dotenv;
use libsql::Builder;
use std::env;

use crate::channel::LuxChannelData;
use crate::error::Error;

use uuid::Uuid;

pub async fn connect() -> Result<libsql::Connection, Error> {
    dotenv().expect(".env file not found");

    let db_path = env::var("DB_PATH").map_err(Error::from)?;
    let sync_url = env::var("TURSO_SYNC_URL").map_err(Error::from)?;
    let auth_token = env::var("TURSO_AUTH_TOKEN").map_err(Error::from)?;

    let db = Builder::new_remote_replica(db_path, sync_url, auth_token)
        .build()
        .await
        .unwrap();

    let conn = db.connect().unwrap();

    print!("Syncing with remote database...");
    db.sync().await.unwrap();
    println!(" done");

    Ok(conn)
}

#[tauri::command]
pub async fn delete_channels() {
    log::debug!["server received delete_channels command"];
    let conn = connect().await.unwrap();

    conn.execute("DELETE FROM channels", ()).await.unwrap();
}

pub async fn _create_tables() -> Result<(), libsql::Error> {
    let conn = connect().await.unwrap();

    conn.execute_batch(
        r#"
            CREATE TABLE IF NOT EXISTS buffer ( 
                id UUID PRIMARY KEY, 
                data BYTEA NOT NULL 
            );
            CREATE TABLE IF NOT EXISTS channels ( 
                id UUID PRIMARY KEY, 
                disabled BOOLEAN NOT NULL, 
                channel_number INTEGER NOT NULL UNIQUE, 
                label VARCHAR(255) NOT NULL, 
                label_color VARCHAR(255) NOT NULL 
            );
        "#,
    )
    .await
}

#[tauri::command]
pub async fn insert_channel(
    disabled: bool,
    channel_number: u8,
    label: String,
    label_color: String,
) -> u64 {
    log::debug!["server received insert_channel command"];
    let conn = connect().await.unwrap();
    let id = Uuid::new_v4().to_string();

    let result = conn
        .execute(
            r#"INSERT INTO channels (
            id, disabled, channel_number, label, label_color
        ) VALUES (
            ?, ?, ?, ?, ?
        )"#,
            (id, disabled, channel_number, label, label_color),
        )
        .await;

    result.unwrap()
}

#[tauri::command]
pub async fn get_initial_state() -> Result<Vec<LuxChannelData>, Error> {
    log::debug!["server received get_initial_state command"];
    let conn = connect().await.unwrap();

    let mut results = conn
        .query("SELECT * FROM channels ORDER BY channel_number", ())
        .await
        .unwrap();
    let mut items: Vec<LuxChannelData> = Vec::new();

    while let Some(row) = results.next().await.unwrap() {
        let id_str: String = row.get(0).unwrap();
        let channel = LuxChannelData {
            id: uuid::Uuid::parse_str(&id_str).unwrap(),
            disabled: row.get(1).unwrap(),
            channel_number: row.get::<u32>(2).unwrap() as usize,
            label: row.get(3).unwrap(),
            label_color: row.get::<String>(4).unwrap().parse().unwrap(),
        };
        items.push(channel);
    }
    log::info!("sqlite server responded: {:?}", items);

    Ok(items)
}
