use crate::{
    buffer::{Buffer, LuxBuffer},
    channels::LuxChannels,
};

use tauri::{AppHandle, Manager, Runtime};

#[taurpc::procedures(path = "sync", event_trigger = SyncEventTrigger)]
pub trait SyncMethods {
    async fn sync_buffer<R: Runtime>(app_handle: AppHandle<R>) -> Result<LuxBuffer, String>;
    async fn sync_channels<R: Runtime>(app_handle: AppHandle<R>) -> Result<LuxChannels, String>;
    async fn sync_state<R: Runtime>(app_handle: AppHandle<R>) -> Result<String, String>;

    #[taurpc(event)]
    async fn buffer_set(buffer: Buffer);
}

#[derive(Clone)]
pub struct SyncEndpoint;

#[taurpc::resolvers]
impl SyncMethods for SyncEndpoint {
    async fn sync_buffer<R: Runtime>(self, app_handle: AppHandle<R>) -> Result<LuxBuffer, String> {
        log::trace!("sync_buffer");
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        let buffer = state.buffer.lock().as_deref().unwrap().clone();
        state.set(buffer, app_handle.clone())
    }

    async fn sync_channels<R: Runtime>(
        self,
        app_handle: AppHandle<R>,
    ) -> Result<LuxChannels, String> {
        log::trace!("sync_channels");
        let mut state = app_handle.state::<LuxChannels>().get_all();
        state.set_channels(LuxChannels::from(&state), app_handle.clone())
    }

    async fn sync_state<R: Runtime>(self, app_handle: AppHandle<R>) -> Result<String, String> {
        log::trace!("sync_state");
        SyncEndpoint.sync_buffer(app_handle.clone()).await?;
        SyncEndpoint.sync_channels(app_handle).await?;
        let msg = format!("State synced!");
        log::trace!("{:?}", msg);
        Ok(msg)
    }
}
