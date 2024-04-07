use crate::{
    buffer::{Buffer, LuxBuffer},
    channel::LuxChannel,
    channels::LuxChannels,
};
use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};
use tauri::{command, State};

#[command]
pub fn set_buffer(
    buffer: Buffer,
    app: tauri::AppHandle,
    state: State<'_, LuxBuffer>,
) -> Result<LuxBuffer, String> {
    log::trace!("received buffer {:?}", buffer);
    let mut state = state.inner().clone();
    state.set(buffer, app)
}

#[command]
pub fn update_channel_value(
    channel_number: usize,
    value: u8,
    app: tauri::AppHandle,
    state: State<'_, LuxBuffer>,
) -> Result<LuxBuffer, String> {
    log::debug!("received channel {} to {}", channel_number, value);
    let mut state = state.inner().clone();
    state.set_channel(channel_number, value, app)
}

#[command]
pub fn update_channel_metadata(
    channel_number: usize,
    new_metadata: LuxChannel,
    app: tauri::AppHandle,
    state_mutex: State<'_, Arc<Mutex<LuxChannels>>>,
) -> Result<LuxChannel, String> {
    log::trace!("received channel {:?}", new_metadata);
    let mut state = state_mutex.lock().unwrap();

    state.set(channel_number, new_metadata, app)
}

#[command]
pub fn sync_state(app: tauri::AppHandle) -> Result<String, String> {
    log::trace!("sync_state");
    crate::sync::sync_state(&app.clone())
}
