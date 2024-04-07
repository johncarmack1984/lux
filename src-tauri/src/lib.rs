// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code, unused_imports, unused_variables)]
mod buffer;
mod channel;
mod channels;
mod cmd;
mod colors;
mod devices;
mod error;
mod http;
mod logger;
mod sync;

use crate::http::get_ngrok_domain;
use axum::response::IntoResponse;
use axum::Json;
use axum::{extract::ConnectInfo, routing::get, Router};
use buffer::{Buffer, LuxBuffer};
use channels::LuxChannels;
use http::secure_tunnel;
use hyper::StatusCode;
use ngrok::prelude::*;
use ngrok::Tunnel;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[allow(dead_code)]
struct AsyncProcInputTx {
    inner: Mutex<mpsc::Sender<String>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    // #[cfg(debug_assertions)] // only enable instrumentation in development builds
    // let devtools = tauri_plugin_devtools::init();

    let builder = tauri::Builder::default();

    // #[cfg(debug_assertions)]
    // let builder = builder.plugin(devtools);

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(logger::logger().build())
        .manage(LuxBuffer::default())
        .manage(LuxChannels::default())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            secure_tunnel(app);
            Ok(())
        })
        // .plugin(tauri_plugin_notification::init())
        // .plugin(tauri_plugin_cli::init())
        // .setup(|app| crate::http::setup_http(app))
        .invoke_handler(tauri::generate_handler![
            cmd::update_channel_value,
            cmd::set_buffer,
            cmd::sync_state,
            cmd::update_channel_metadata,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}

async fn async_process_model(
    mut input_rx: mpsc::Receiver<String>,
    output_tx: mpsc::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    while let Some(input) = input_rx.recv().await {
        let output = input;
        output_tx.send(output).await?;
    }

    Ok(())
}
