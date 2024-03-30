use crate::{buffer::LuxBuffer, sync};
use std::sync::{Arc, Mutex};

use crate::state::LuxState;
use tauri::{command, State};

#[command]
pub fn set_buffer(
    buffer: LuxBuffer,
    app: tauri::AppHandle,
    state_mutex: State<'_, Arc<Mutex<LuxState>>>,
) -> Result<LuxBuffer, String> {
    log::trace!("received buffer {:?}", buffer);
    let mut state = state_mutex.lock().unwrap();

    Ok(state.set_buffer(buffer, app).unwrap())
}

#[command]
pub fn update_channel_value(
    channel_number: usize,
    value: u8,
    app: tauri::AppHandle,
    state_mutex: State<'_, Arc<Mutex<LuxState>>>,
) -> Result<LuxBuffer, String> {
    log::trace!("received channel {} to {}", channel_number, value);
    let mut state = state_mutex.lock().unwrap();

    Ok(state.set_channel_value(channel_number, value, app).unwrap())
}

#[command]
pub fn sync_state(app: tauri::AppHandle) {
    log::trace!("sync_state");
    sync::sync_state(&app.clone());
}
