use std::sync::{Arc, Mutex};
use tauri::State;

use crate::LuxState;

use enttecopendmx;

#[tauri::command]
pub fn update(
    channel: usize,
    value: u8,
    window: tauri::Window,
    state_mutex: State<'_, Arc<Mutex<LuxState>>>,
) -> Result<LuxState, String> {
    let mut state = state_mutex.lock().unwrap();
    state.buffer[channel - 1] = value;
    window.emit("system_state_update", state.clone()).unwrap();

    let mut temp_buffer = [0; 513];

    temp_buffer[1..7].copy_from_slice(&state.buffer);

    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_buffer(temp_buffer);
    interface.render().unwrap();
    interface.close().unwrap();
    Ok(state.clone())
}

#[tauri::command]
pub fn full_bright(
    window: tauri::Window,
    state_mutex: State<'_, Arc<Mutex<LuxState>>>,
) -> Result<LuxState, String> {
    let mut state = state_mutex.lock().unwrap();
    state.buffer = [255; lux::BUFFER_SIZE];
    window.emit("system_state_update", state.clone()).unwrap();
    let mut temp_buffer = [0; 513];
    temp_buffer[1..7].copy_from_slice(&state.buffer);

    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_buffer(temp_buffer);
    interface.render().unwrap();
    interface.close().unwrap();
    Ok(state.clone())
}

#[tauri::command]
pub fn blackout(
    window: tauri::Window,
    state_mutex: State<'_, Arc<Mutex<LuxState>>>,
) -> Result<LuxState, String> {
    let mut state = state_mutex.lock().unwrap();
    state.buffer = [0; lux::BUFFER_SIZE];
    window.emit("system_state_update", state.clone()).unwrap();
    let mut temp_buffer = [0; 513];
    temp_buffer[1..7].copy_from_slice(&state.buffer);
    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_buffer(temp_buffer);
    interface.render().unwrap();
    interface.close().unwrap();
    Ok(state.clone())
}

#[tauri::command]
pub fn _rgb_chase(_window: tauri::Window, _state_mutex: State<'_, Arc<Mutex<LuxState>>>) {
    use std::thread;
    use std::time::Duration;
    const SLEEPTIME: u64 = 100;
    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_channel(6, 255);
    loop {
        for i in 1..4 {
            interface.set_channel(i as usize, 255 as u8);
            // interface.buffer[1] = interface.buffer[1] + 10;
            interface.render().unwrap();
            interface.set_channel(i as usize, 0 as u8);
            thread::sleep(Duration::from_millis(SLEEPTIME));
        }
    }
}
