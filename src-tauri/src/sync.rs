// Migrated from TauRPC by ttipc-migrate. Manual follow-ups:
//   - errors: each `Result<_, E>` needs `E: ttipc::Error` (derive it on the error type).
//   - imports: drop the now-unused `Runtime`; `Channel` is now `ttipc::Channel`.
//   - de-async: methods with no blocking `.await` were made sync (ttipc's default), and their `.await`s on now-sync siblings were dropped.
//   - events: `#[taurpc(event)]` methods were lifted into a `#[derive(ttipc::Event)]` enum (matching emit sites were rewritten to `Enum::Variant.emit(&h)`); drop any now-empty trait/impl.

use crate::{
    buffer::{Buffer, LuxBuffer},
    channels::LuxChannels,
};

use tauri::{AppHandle, Manager, Runtime};

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
        let buffer = state.buffer.lock().as_deref().unwrap().clone();
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
        let msg = format!("State synced!");
        log::trace!("{:?}", msg);
        Ok(msg)
    }
}
