use crate::{buffer::LuxBuffer, channel::LuxChannels, state::LuxState};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

pub fn sync_buffer(app_handle: &AppHandle) -> Result<LuxBuffer, String> {
    log::trace!("sync_buffer");
    let state_mutex: State<'_, Arc<Mutex<LuxState>>> = app_handle.state();
    let mut state = state_mutex.lock().unwrap();
    let buffer = state.buffer.clone();
    let app_handle: AppHandle = app_handle.clone();
    state.set_buffer(buffer, app_handle)
}

pub fn sync_channels(app_handle: &AppHandle) -> Result<LuxChannels, String> {
    log::trace!("sync_channels");
    let state_mutex: State<'_, Arc<Mutex<LuxState>>> = app_handle.state();
    let mut state = state_mutex.lock().unwrap();
    let channels = state.channels.clone();
    let app_handle: AppHandle = app_handle.clone();
    state.set_channels(channels, app_handle)
}

pub fn sync_state(app_handle: &AppHandle) -> Result<String, String> {
    log::trace!("sync_state");
    sync_buffer(app_handle)?;
    sync_channels(app_handle)?;
    let msg = format!("State synced!");
    log::trace!("{:?}", msg);
    Ok(msg)
}
