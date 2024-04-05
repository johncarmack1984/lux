use crate::{
    buffer::{Buffer, LuxBuffer},
    channels::LuxChannels,
};

use tauri::{AppHandle, Manager};

pub fn sync_buffer(app_handle: &AppHandle) -> Result<LuxBuffer, String> {
    log::trace!("sync_buffer");
    let mut state: LuxBuffer = app_handle.state::<LuxBuffer>().get();
    state.set(Buffer::from(&state), app_handle.clone())
}

pub fn sync_channels(app_handle: &AppHandle) -> Result<LuxChannels, String> {
    log::trace!("sync_channels");
    let mut state = app_handle.state::<LuxChannels>().get_all();
    state.set_channels(LuxChannels::from(&state), app_handle.clone())
}

pub fn sync_state(app_handle: &AppHandle) -> Result<String, String> {
    log::trace!("sync_state");
    sync_buffer(app_handle)?;
    sync_channels(app_handle)?;
    let msg = format!("State synced!");
    log::trace!("{:?}", msg);
    Ok(msg)
}
