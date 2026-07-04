use crate::lock::LockPolicy;
use crate::{
    buffer::{Buffer, LuxBuffer},
    channels::LuxChannels,
};

use tauri::{AppHandle, Manager};

#[ttipc::procedures(path = "sync")]
pub trait SyncMethods {
    fn sync_buffer(&self, app_handle: AppHandle) -> Result<LuxBuffer, String>;
    fn sync_channels(&self, app_handle: AppHandle) -> Result<LuxChannels, String>;
    fn sync_state(&self, app_handle: AppHandle) -> Result<String, String>;
}
#[derive(ttipc::Event)]
pub enum SyncEvent {
    BufferSet { buffer: Buffer },
}

#[derive(Clone)]
pub struct SyncEndpoint;

impl SyncMethods for SyncEndpoint {
    fn sync_buffer(&self, app_handle: AppHandle) -> Result<LuxBuffer, String> {
        log::trace!("sync_buffer");
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        let buffer = state.buffer.lock_or_recover().clone();
        state.set(buffer, app_handle.clone())
    }
    fn sync_channels(&self, app_handle: AppHandle) -> Result<LuxChannels, String> {
        log::trace!("sync_channels");
        let mut state = app_handle.state::<LuxChannels>().get_all();
        state.set_channels(LuxChannels::from(&state), app_handle.clone())
    }
    fn sync_state(&self, app_handle: AppHandle) -> Result<String, String> {
        log::trace!("sync_state");
        SyncEndpoint.sync_buffer(app_handle.clone())?;
        SyncEndpoint.sync_channels(app_handle)?;
        let msg = "State synced!".to_string();
        log::trace!("{:?}", msg);
        Ok(msg)
    }
}
