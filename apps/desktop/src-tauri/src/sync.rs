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
    /// Read the live buffer. **A read, and only a read.**
    ///
    /// This used to round-trip through `LuxBuffer::set`, which is the write
    /// path: it renders to the DMX output and republishes the retained state
    /// echo. Every caller here wants the current value — the mount-time query
    /// behind `useBuffer`, the preset toggle's base snapshot — so those were
    /// side effects nobody asked for, on a code path that runs whenever a
    /// surface opens.
    ///
    /// That is worse than wasted work. A device that has been asleep holds a
    /// stale buffer, so opening the app pushed *that* onto the rig: rendering
    /// it over sACN, and overwriting the retained echo every other surface
    /// reads as truth. With one person driving it usually went unnoticed,
    /// because the stale buffer was their own last state. It stops being
    /// survivable the moment someone else is at the desk.
    fn sync_buffer(&self, app_handle: AppHandle) -> Result<LuxBuffer, String> {
        log::trace!("sync_buffer");
        Ok(app_handle.state::<LuxBuffer>().inner().clone())
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
