use log;
// use std::sync::{Arc, Mutex};
use tauri::{
    command,
    ipc::CommandScope,
    // State,
    //  Manager,
    //   State
};

// use crate::{
//     buffer::{LuxBuffer, BUFFER_SIZE},
//     channels::LuxChannel,
//     state::LuxState,
// };
use serde::{Deserialize, Serialize};

// use crate::{buffer::LuxBuffer, state::LuxState};

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct RequestBody {
    id: i32,
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct LogScope {
    event: String,
}

#[command]
pub fn log_operation(
    event: String,
    payload: Option<String>,
    command_scope: CommandScope<LogScope>,
) -> Result<(), &'static str> {
    if command_scope.denies().iter().any(|s| s.event == event) {
        Err("denied")
    } else if !command_scope.allows().iter().any(|s| s.event == event) {
        Err("not allowed")
    } else {
        log::info!("{} {:?}", event, payload);
        Ok(())
    }
}

#[derive(Serialize)]
pub struct ApiResponse {
    message: String,
}

#[command]
pub fn perform_request(endpoint: String, body: RequestBody) -> ApiResponse {
    println!("{} {:?}", endpoint, body);
    ApiResponse {
        message: "message response".into(),
    }
}

// #[command]
// pub fn get_buffer(state_mutex: State<'_, Arc<Mutex<LuxState>>>) -> LuxBuffer {
//     log::debug!("Getting buffer data");
//     LuxBuffer(state_mutex.lock().unwrap().buffer.clone())
// }

// #[command]
// pub fn get_channel_data(state_mutex: State<'_, Arc<Mutex<LuxState>>>) -> [LuxChannel; BUFFER_SIZE] {
//     log::debug!("Getting channel data");
//     state_mutex.lock().unwrap().channels.clone()
// }

// #[command]
// pub fn pulse_channel_data(
//     app: tauri::AppHandle,
//     state_mutex: State<'_, Arc<Mutex<LuxState>>>,
// ) -> Result<[LuxChannel; BUFFER_SIZE], String> {
//     log::debug!("Pulsing channel data");
//     app.emit(
//         "channel_data_update",
//         state_mutex.lock().unwrap().channels.clone(),
//     )
//     .unwrap();
//     Ok(state_mutex.lock().unwrap().channels.clone())
// }

// #[command]
// pub fn update_channel_value(
//     channel_number: usize,
//     value: u8,
//     app: tauri::AppHandle,
//     state_mutex: State<'_, Arc<Mutex<LuxState>>>,
// ) -> Result<LuxBuffer, String> {
//     log::debug!("Updating channel {} to {}", channel_number, value);
//     let mut state = state_mutex.lock().unwrap();
//     state.set_channel(channel_number, value).unwrap();
//     app.emit("buffer_update", state.clone().buffer).unwrap();
//     Ok(LuxBuffer(state.clone().buffer))
// }

// #[command]
// pub fn full_bright(
//     app: tauri::AppHandle,
//     state_mutex: State<'_, Arc<Mutex<LuxState>>>,
// ) -> Result<LuxBuffer, String> {
//     log::debug!("Setting full bright");
//     let mut state = state_mutex.lock().unwrap();
//     state.full_bright().unwrap();
//     // app.emit("buffer_update", state.clone().buffer).unwrap();
//     Ok(LuxBuffer(state.clone().buffer))
// }

#[allow(dead_code)]
#[command]
pub fn full_bright() {
    println!("Setting full bright");
}

// #[command]
// pub fn blackout(
//     app: tauri::AppHandle,
//     state_mutex: State<'_, Arc<Mutex<LuxState>>>,
// ) -> Result<LuxBuffer, String> {
//     log::debug!("Setting blackout");
//     let mut state = state_mutex.lock().unwrap();
//     state.blackout().unwrap();
//     // app.emit("buffer_update", state.clone().buffer).unwrap();
//     Ok(LuxBuffer(state.clone().buffer))
// }

#[allow(dead_code)]
#[command]
pub fn blackout() {
    println!("Setting blackout");
}

// // #[command]
// // pub fn _rgb_chase(_window: tauri::Window, _state_mutex: State<'_, Arc<Mutex<LuxState>>>) {
// //     use std::thread;
// //     use std::time::Duration;
// //     const SLEEPTIME: u64 = 100;
// //     let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
// //     interface.open().unwrap();
// //     interface.set_channel(6, 255);
// //     loop {
// //         for i in 1..4 {
// //             interface.set_channel(i as usize, 255 as u8);
// //             // interface.buffer[1] = interface.buffer[1] + 10;
// //             interface.render().unwrap();
// //             interface.set_channel(i as usize, 0 as u8);
// //             thread::sleep(Duration::from_millis(SLEEPTIME));
// //         }
// //     }
// // }
