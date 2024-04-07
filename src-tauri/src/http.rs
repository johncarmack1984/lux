#![allow(dead_code, unused_imports, unused_variables)]
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{extract::ConnectInfo, routing::get, Router};
use ngrok::prelude::*;
use std::sync::Mutex;
#[allow(dead_code)]
use tauri::Manager;
use tauri::{App, AppHandle};
use tokio::sync::mpsc;

use crate::buffer::LuxBuffer;

pub async fn setup_http(app: &mut App) -> anyhow::Result<()> {
    let app_handle = app.handle();
    // let app_handle_clone = app_handle.clone();
    // tauri::async_runtime::spawn(async move {
    // loop {
    //         if let Some(output) = async_proc_output_rx.recv().await {
    //             app_handle_clone
    //                 .emit("incoming_api_request", Some(output))
    //                 .unwrap();
    //         }
    // }
    // });
    // let app_handle_clone = app_handle.clone();
    // tauri::async_runtime::spawn(async move { async_setup_http(app_handle_clone) });

    Ok(())
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

// async fn async_setup_http(_app: AppHandle) -> Result<(), BoxError> {
//     use dotenvy::dotenv;
//     dotenv().expect(".env file not found");

//     let router: Router = Router::new().route(
//         "/",
//         get(
//             |ConnectInfo(remote_addr): ConnectInfo<SocketAddr>| async move {
//                 format!("Hello, {remote_addr:?}!\r\n")
//             },
//         ), // .post(move |body| set_buffer(body, app_handle, state)),
//     );

//     let mut listener = ngrok::Session::builder()
//         .authtoken_from_env()
//         .connect()
//         .await?
//         .http_endpoint()
//         .metadata("example tunnel metadata from rust")
//         // .scheme(Scheme::HTTP)
//         .domain(get_ngrok_domain())
//         // .forwards_to("3003")
//         // .oauth(OauthOptions::new("google").allow_email("johncarmack@me.com"))
//         .listen()
//         .await?;

//     log::info!("Listener started on URL: {:?}", listener.url());

//     let mut make_service = router.into_make_service_with_connect_info::<SocketAddr>();

//     let server = async move {
//         while let Some(conn) = listener.try_next().await? {
//             let remote_addr = conn.remote_addr();
//             let tower_service = unwrap_infallible(make_service.call(remote_addr).await);

//             tokio::spawn(async move {
//                 let hyper_service =
//                     hyper::service::service_fn(move |request: Request<Incoming>| {
//                         tower_service.clone().oneshot(request)
//                     });

//                 //     if let Err(err) = server::conn::auto::Builder::new(TokioExecutor::new())
//                 //         .serve_connection(conn, hyper_service)
//                 //         // .serve_connection_with_upgrades(conn, hyper_service)
//                 //         .await
//                 //     {
//                 //         log::error!("server error: {err:#}");
//                 //     }
//             });
//         }
//         Ok::<(), BoxError>(())
//     };

//     server.await?;

//     Ok(())
// }

pub fn axum_router(app: AppHandle) -> Router {
    let app_handle = Arc::new(app);
    let _state = Arc::new(app_handle.state::<LuxBuffer>().get());
    Router::new().route(
        "/",
        get(
            |ConnectInfo(remote_addr): ConnectInfo<SocketAddr>| async move {
                format!("Hello, {remote_addr:?}!\r\n")
            },
        ), // .post(move |body| set_buffer(body, app_handle, state)),
    )
}

pub fn get_ngrok_domain() -> String {
    use dotenvy::dotenv;
    dotenv().expect(".env file not found");
    std::env::var("NGROK_TUNNEL_DOMAIN")
        .map_err(crate::error::Error::from)
        .unwrap()
}

// async fn get_buffer(// app: Arc<AppHandle>,
//     // state: Arc<LuxBuffer>,
// ) -> impl IntoResponse {
//     // let buffer = state.get().get().unwrap();
//     // let msg = format!("buffer: {:?}", buffer);
//     // (StatusCode::OK, Json(msg))
//     (StatusCode::OK, Json("buffer".to_string()))
// }

// async fn set_buffer(
//     Json(body): Json<Buffer>,
//     app: Arc<AppHandle>,
//     state: Arc<LuxBuffer>,
// ) -> impl IntoResponse {
//     log::debug!("body {:?}", body);
//     log::debug!("state {:?}", state);
//     app.emit("incoming_api_request", body.clone()).unwrap();
//     let app_handle = app.app_handle().clone();
//     state.get().set(body, app_handle).unwrap();
//     let msg = format!("buffer: {:?}", body);
//     (StatusCode::OK, Json(msg))
// }

// let router = axum_router(app_handle_clone);

// let addr = "localhost:3003";
// let _tunnel = lux_tunnel(addr).await.unwrap();
// let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

// axum::serve(
//     listener,
//     router.into_make_service_with_connect_info::<SocketAddr>(),
// )
// .await
// .unwrap();
