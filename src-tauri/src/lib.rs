// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod account;
mod buffer;
mod channel;
mod channels;
mod cmd;
mod colors;
mod devices;
mod error;
mod fixture;
mod logger;
mod remote;
mod setup;
mod sync;
mod tray;

use buffer::LuxBuffer;
use channels::LuxChannels;
use cmd::*;
use devices::DmxOutput;
use sync::*;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    // rustls 0.23 (pulled by rumqttc + reqwest) needs a process-level crypto
    // provider installed before any TLS use, or it panics. Install ring explicitly.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = setup(tauri::Builder::default(), |app| {
        // Load the user's setups (migrating a legacy patch.json, or seeding the
        // default "Home" setup on first run) into state.
        app.manage(setup::load(app.handle()));
        // Cognito accounts (no-op unless COGNITO_* is set); restore a signed-in
        // session from the keychain in the background.
        app.manage(account::LuxAccount::from_env());
        account::restore_on_startup(app.handle());
        remote::connect(app.handle());
        if let Err(e) = tray::build(app) {
            log::error!("tray setup failed: {e}");
        }
        // Auto-detect DMX devices (USB + network), select one, and populate the
        // tray (retries through cold-start); then keep the network node fed.
        devices::start_autodetect(app.handle());
        devices::start_keepalive(app.handle());
    });
    let default_buffer = LuxBuffer::from(vec![121, 255, 255, 0, 0, 42]);
    let default_channels = LuxChannels::default();
    let router = SyncEndpoint
        .into_procedures()
        .merge(CmdEndpoint.into_procedures());
    let taurpc = ttipc::handler(router);
    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(logger::logger().build())
        .manage(default_buffer)
        .manage(default_channels)
        .manage(DmxOutput::default())
        .invoke_handler(taurpc)
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}

pub fn setup<R, F>(builder: tauri::Builder<R>, setup: F) -> tauri::Builder<R>
where
    R: tauri::Runtime,
    F: FnOnce(&tauri::App<R>) + Send + 'static,
{
    builder.setup(move |app| Ok(setup(app)))
}

pub fn ttipc_bindings() -> ttipc::Bindings {
    ttipc::Bindings::new()
        .method_case(ttipc::MethodCase::Snake)
        .router("createTauRPCProxy")
        .register::<CmdMethodsProcedures>()
        .register::<SyncMethodsProcedures>()
        .register_events::<CmdEvent>()
        .register_events::<SyncEvent>()
}
