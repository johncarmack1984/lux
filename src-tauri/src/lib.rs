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
use axum::{extract::ConnectInfo, routing::get, Router};
use buffer::LuxBuffer;
use channels::LuxChannels;
use ngrok::prelude::*;
use ngrok::Tunnel;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[allow(dead_code)]
struct AsyncProcInputTx {
    inner: Mutex<mpsc::Sender<String>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    #[allow(unused_variables)]
    let (async_proc_input_tx, async_proc_input_rx) = mpsc::channel(1);
    #[allow(unused_mut)]
    let (async_proc_output_tx, mut async_proc_output_rx) = mpsc::channel(1);

    tokio::spawn(
        async move { async_process_model(async_proc_input_rx, async_proc_output_tx).await },
    );

    // tokio::spawn(async move {
    //     // let app_handle = app.handle();
    //     // let app_handle_clone = app_handle.clone();
    //     // let async_proc_input_tx_clone = async_proc_input_tx.clone();
    //     tauri::async_runtime::spawn(async move {
    //         use dotenvy::dotenv;
    //         dotenv().expect(".env file not found");
    //         let router: Router = Router::new().route(
    //             "/",
    //             get("hello"), // .post(move |body| set_buffer(body, app_handle, state)),
    //         );
    //         let sess = ngrok::Session::builder()
    //             .authtoken_from_env()
    //             .connect()
    //             .await
    //             .unwrap();

    //         let tun = sess
    //             .http_endpoint()
    //             .domain(get_ngrok_domain())
    //             .listen_and_forward("http://0.0.0.0:3003".parse().unwrap())
    //             .await
    //             .unwrap();

    //         log::info!("Listener started on URL: {:?}", tun.url());

    //         axum::Server::bind(&"0.0.0.0:3003".parse().unwrap())
    //             .serve(router.into_make_service())
    //             .await
    //             .unwrap();
    //         // axum::Server::builder(start_tunnel().await.unwrap())
    //         // async_setup_http(app_handle_clone, async_proc_input_tx_clone).await
    //     });
    // });

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
            tokio::spawn(async move {
                // let app_handle = app.handle();
                // let app_handle_clone = app_handle.clone();
                // let async_proc_input_tx_clone = async_proc_input_tx.clone();
                tauri::async_runtime::spawn(async move {
                    use dotenvy::dotenv;
                    dotenv().expect(".env file not found");
                    let router: Router = Router::new().route(
                        "/",
                        get("hello"), // .post(move |body| set_buffer(body, app_handle, state)),
                    );
                    let sess = ngrok::Session::builder()
                        .authtoken_from_env()
                        .connect()
                        .await
                        .unwrap();

                    let tun = sess
                        .http_endpoint()
                        .domain(get_ngrok_domain())
                        .listen_and_forward("http://0.0.0.0:3003".parse().unwrap())
                        .await
                        .unwrap();

                    log::info!("Listener started on URL: {:?}", tun.url());

                    axum::Server::bind(&"0.0.0.0:3003".parse().unwrap())
                        .serve(router.into_make_service())
                        .await
                        .unwrap();
                    // axum::Server::builder(start_tunnel().await.unwrap())
                    // async_setup_http(app_handle_clone, async_proc_input_tx_clone).await
                });
            });
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
