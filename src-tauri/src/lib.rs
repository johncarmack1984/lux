// use state::lux_state;
// use tauri::ipc::Channel;
// use tauri::webview::PageLoadEvent;
// use tauri::Manager;
// use tauri::WebviewUrl;
// use tauri::WebviewWindowBuilder;
mod buffer;
mod channels;
mod cmd;
mod colors;
mod devices;
mod error;
mod logger;
#[cfg(desktop)]
mod menu_plugin;
// mod rpc;
mod state;
#[cfg(desktop)]
mod tray;

// use rpc::ApiImpl;
use serde::Serialize;

use tauri::{App, AppHandle, RunEvent, Runtime};

pub type SetupHook = Box<dyn FnOnce(&mut App) -> Result<(), Box<dyn std::error::Error>> + Send>;
pub type OnEvent = Box<dyn FnMut(&AppHandle, RunEvent)>;

// use crate::__cmd__update_channel_value as update_channel_value;
// use crate::commands::{blackout, full_bright, get_buffer, get_channel_data, pulse_channel_data};
// use crate::cmd::{blackout, full_bright};
// use crate::tray::{system_tray, tray_event_handler};
// use crate::{
//     logger::logger, // , state::lux_state
// };

#[derive(Clone, Serialize)]
struct Reply {
    data: String,
}

#[cfg(target_os = "macos")]
pub struct AppMenu<R: Runtime>(pub std::sync::Mutex<Option<tauri::menu::Menu<R>>>);

#[allow(dead_code)]
#[cfg(desktop)]
pub struct PopupMenu<R: Runtime>(tauri::menu::Menu<R>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_app(
        tauri::Builder::default(), // .plugin(tauri_plugin_http::init())
        // .plugin(tauri_plugin_cli::init())
        // .plugin(logger().build())
        // .plugin(tauri_plugin_notification::init())
        // .invoke_handler(tauri::generate_handler![
        //     cmd::log_operation,
        //     cmd::perform_request,
        //     cmd::full_bright
        // ])
        // .manage(lux_state())
        |_app| {},
    )
}
// pub fn run() -> Result<(), tauri::Error> {
//     tauri::Builder::default()
//         .manage(lux_state())
//         // .system_tray(system_tray())
//         // .on_system_tray_event(tray_event_handler)
//         .invoke_handler(tauri::generate_handler![
//             // blackout,
//             // full_bright,
//             // get_buffer,
//             // get_channel_data,
//             // pulse_channel_data,
//             // update_channel_value
//         ])
//         .run(tauri::generate_context!())
// }

pub fn run_app<R: Runtime, F: FnOnce(&App<R>) + Send + 'static>(
    // #[allow(unused_variables)]
    builder: tauri::Builder<R>,
    #[allow(unused_variables)] setup: F,
) {
    #[allow(unused_mut)]
    let mut builder = builder
        // .run(tauri::generate_context!())
        // builder.run(tauri::generate_context!())
        // .invoke_handler(generate_handler![full_bright])
        // .invoke_handler(tauri::generate_handler![blackout, full_bright])
        // .plugin(tauri_plugin_sample::init())
        .setup(move |app| {
            // #[cfg(all(desktop, not(test)))]
            // {
            // let handle = app.handle();
            //         tray::create_tray(handle)?;
            //         handle.plugin(menu_plugin::init())?;
            //     }

            //     #[cfg(target_os = "macos")]
            //     app.manage(AppMenu::<R>(Default::default()));

            //     #[cfg(all(desktop, not(test)))]
            //     app.manage(PopupMenu(
            //         tauri::menu::MenuBuilder::new(app)
            //             .check("check", "Tauri is awesome!")
            //             .text("text", "Do something")
            //             .copy()
            //             .build()?,
            //     ));

            //     // let mut window_builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default());

            //     // #[cfg(all(desktop, not(test)))]
            //     // {
            //     //     window_builder = window_builder
            //     //         .title("Tauri API Validation")
            //     //         .inner_size(1000., 800.)
            //     //         .min_inner_size(600., 400.)
            //     //         .content_protected(true)
            //     //         .menu(tauri::menu::Menu::default(app.handle())?);
            //     // }

            //     // let webview = window_builder.build()?;

            //     // #[cfg(debug_assertions)]
            //     // webview.open_devtools();

            //     // let value = Some("test".to_string());
            //     // let response = app.sample().ping(PingRequest {
            //     //     value: value.clone(),
            //     //     on_event: Channel::new(|event| {
            //     //         println!("got channel event: {:?}", event);
            //     //         Ok(())
            //     //     }),
            //     // });
            //     // log::info!("got response: {:?}", response);
            //     // if let Ok(res) = response {
            //     //     assert_eq!(res.value, value);
            //     // }

            //     #[cfg(desktop)]
            //     std::thread::spawn(|| {
            //         let server = match tiny_http::Server::http("localhost:3003") {
            //             Ok(s) => s,
            //             Err(e) => {
            //                 eprintln!("{}", e);
            //                 std::process::exit(1);
            //             }
            //         };
            //         loop {
            //             if let Ok(mut request) = server.recv() {
            //                 let mut body = Vec::new();
            //                 let _ = request.as_reader().read_to_end(&mut body);
            //                 let response = tiny_http::Response::new(
            //                     tiny_http::StatusCode(200),
            //                     request.headers().to_vec(),
            //                     std::io::Cursor::new(body),
            //                     request.body_length(),
            //                     None,
            //                 );
            //                 let _ = request.respond(response);
            //             }
            //         }
            // });

            // setup(app);
            Ok(())
        })
        .on_page_load(|webview, payload| {
            //     if payload.event() == PageLoadEvent::Finished {
            //         let webview_ = webview.clone();
            //         webview.listen("js-event", move |event| {
            //             println!("got js-event with message '{:?}'", event.payload());
            //             let reply = Reply {
            //                 data: "something else".to_string(),
            //             };

            //             webview_
            //                 .emit("rust-event", Some(reply))
            //                 .expect("failed to emit");
            //         });
            //     }
        });

    #[allow(unused_mut)]
    let mut app = builder
        //     .invoke_handler(tauri::generate_handler![
        //         cmd::log_operation,
        //         cmd::perform_request,
        //         cmd::full_bright
        //     ])
        // .invoke_handler(taurpc::create_ipc_handler(ApiImpl).into_handler())
        .build(tauri::tauri_build_context!())
        // .build(tauri::generate_context!())
        .expect("error while building tauri application");

    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Regular);

    app.run(move |_app_handle, _event| {
        #[cfg(all(desktop, not(test)))]
        match &_event {
            //     RunEvent::ExitRequested { api, code, .. } => {
            //         // Keep the event loop running even if all windows are closed
            //         // This allow us to catch tray icon events when there is no window
            //         // if we manually requested an exit (code is Some(_)) we will let it go through
            //         if code.is_none() {
            //             api.prevent_exit();
            //         }
            //     }
            //     RunEvent::WindowEvent {
            //         event: tauri::WindowEvent::CloseRequested { api, .. },
            //         // label,
            //         ..
            //     } => {
            //         println!("closing window...");
            //         // run the window destroy manually just for fun :)
            //         // usually you'd show a dialog here to ask for confirmation or whatever
            //         api.prevent_close();
            //         // _app_handle
            //         //     .get_webview_window(label)
            //         //     .unwrap()
            //         //     .destroy()
            //         //     .unwrap();
            //     }
            _ => (),
        }
    })
}

// #[cfg(test)]
// mod tests {
//     use tauri::Manager;

//     #[test]
//     fn run_app() {
//         super::run_app(tauri::test::mock_builder(), |app| {
//             let window = app.get_webview_window("main").unwrap();
//             std::thread::spawn(move || {
//                 std::thread::sleep(std::time::Duration::from_secs(1));
//                 window.close().unwrap();
//             });
//         })
//     }
// }
