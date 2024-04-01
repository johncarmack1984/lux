mod buffer;
mod channel;
mod cmd;
mod colors;
mod db;
mod devices;
mod error;
mod logger;
mod positioner;
mod state;
mod sync;

use crate::state::LuxState;
use tauri::{App, RunEvent, Runtime};
// use tokio::sync::oneshot;

// use std::time::Duration;

// #[taurpc::procedures]
// trait Api {
//     async fn hello_world();
// }

// #[derive(Clone)]
// struct ApiImpl;

// #[taurpc::resolvers]
// impl Api for ApiImpl {
//     async fn hello_world(self) {
//         println!("Hello, world!");
//     }
// }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    // let (tx, rx) = oneshot::channel::<AppHandle>();

    // tokio::spawn(async move {
    // let app_handle = rx.await.unwrap();
    // let api_trigger = ApiEventTrigger::new(app_handle.clone());
    // let events_trigger = TauRpcEventsEventTrigger::new(app_handle);

    // let mut interval = tokio::time::interval(Duration::from_secs(1));
    // loop {
    //     interval.tick().await;

    //     api_trigger
    //         .send_to(Windows::One("main".to_string()))
    //         .update_state("message scoped".to_string())?;

    //     api_trigger.update_state("message".to_string())?;

    //     events_trigger.vec_test(vec![String::from("test"), String::from("test2")])?;

    //     events_trigger.multiple_args(0, vec![String::from("test"), String::from("test2")])?;

    //     events_trigger.test_ev()?;
    // }

    // Ok::<(), tauri::Error>(())
    // });
    run_app(
        tauri::Builder::default()
            .plugin(tauri_plugin_notification::init())
            .plugin(tauri_plugin_cli::init())
            .plugin(logger::logger().build())
            .plugin(tauri_plugin_positioner::init())
            .setup(|app| Ok(positioner::setup(app)))
            .plugin(tauri_plugin_shell::init())
            .plugin(tauri_plugin_window_state::Builder::default().build())
            .manage(LuxState::default().mutex())
            .invoke_handler(tauri::generate_handler![
                cmd::update_channel_value,
                cmd::set_buffer,
                cmd::sync_state,
                db::remote::insert_channel,
                db::remote::get_initial_state,
                db::remote::delete_channels,
                cmd::update_channel_metadata,
            ]),
        move |_| (),
    )
}

pub fn run_app<R: Runtime, F: FnOnce(&App<R>) + Send + 'static>(
    builder: tauri::Builder<R>,
    _setup: F,
) {
    let app = builder
        // .invoke_handler(taurpc::create_ipc_handler(ApiImpl.into_handler()))
        .build(tauri::tauri_build_context!())
        // .run(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(move |_app_handle, event| match event {
        RunEvent::MainEventsCleared { .. } => {
            return;
        }
        _ => {
            log::trace!("event: {:?}", event);
        }
    })
    // app.run()
}
