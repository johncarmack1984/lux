use crate::buffer::{Buffer, LuxBuffer};
use axum::response::IntoResponse;
use axum::Json;
use axum::{extract::ConnectInfo, routing::get, Router};
use hyper::StatusCode;
use ngrok::prelude::*;
use std::sync::Arc;
use tauri::Manager;

pub fn secure_tunnel(app: &mut tauri::App) {
    let app_handle = app.handle().clone();
    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        tauri::async_runtime::spawn(async move {
            use dotenvy::dotenv;
            dotenv().expect(".env file not found");
            let state = app_handle.state::<LuxBuffer>();
            let router: Router = Router::new()
                .route(
                    "/buffer",
                    get(get_buffer).post(move |body| set_buffer(body, app_handle_clone)),
                )
                .with_state(Arc::new(state.inner().clone()));
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
        });
    });
}

pub fn get_ngrok_domain() -> String {
    use dotenvy::dotenv;
    dotenv().expect(".env file not found");
    std::env::var("NGROK_TUNNEL_DOMAIN")
        .map_err(crate::error::Error::from)
        .unwrap()
}

async fn get_buffer(
    axum::extract::State(state): axum::extract::State<Arc<LuxBuffer>>,
) -> impl IntoResponse {
    let buffer = state.buffer.lock().as_deref().unwrap().clone();
    let msg = format!("buffer: {:?}", buffer);
    (StatusCode::OK, Json(msg))
}

#[derive(serde::Deserialize)]
struct JsonBuffer {
    buffer: Buffer,
}

async fn set_buffer(Json(body): Json<JsonBuffer>, app: tauri::AppHandle) -> impl IntoResponse {
    log::debug!("body {:?}", body.buffer);
    let mut state = app.state::<LuxBuffer>().inner().clone();
    log::debug!("state {:?}", state);
    app.emit("incoming_api_request", body.buffer.clone())
        .unwrap();
    let app_handle = app.app_handle().clone();
    state.set(body.buffer, app_handle).unwrap();
    let msg = format!("buffer: {:?}", body.buffer);
    (StatusCode::OK, Json(msg))
}
