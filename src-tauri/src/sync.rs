use crate::LuxState;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

pub fn sync_buffer(app_handle: &AppHandle) {
    log::trace!("sync_buffer");
    let state_mutex: State<'_, Arc<Mutex<LuxState>>> = app_handle.state();
    let mut state = state_mutex.lock().unwrap();
    let buffer = state.buffer.clone();
    let app_handle: AppHandle = app_handle.clone();
    state.set_buffer(buffer, app_handle).unwrap();
}

pub fn sync_channels(app_handle: &AppHandle) {
    log::trace!("sync_channels");
    let state_mutex: State<'_, Arc<Mutex<LuxState>>> = app_handle.state();
    let mut state = state_mutex.lock().unwrap();
    let channels = state.channels.clone();
    let app_handle: AppHandle = app_handle.clone();
    state.set_channels(channels, app_handle).unwrap();
}

pub fn sync_state(app_handle: &AppHandle) {
    sync_buffer(app_handle);
    sync_channels(app_handle);
    log::trace!("state synced");
}
