use crate::{
    buffer::{Buffer, LuxBuffer, BUFFER_SIZE},
    channel::LuxChannel,
    channels::LuxChannels,
    sync::*,
};
use tauri::{AppHandle, Manager, Runtime};

#[taurpc::procedures(path = "cmd", event_trigger = CmdEventTrigger, export_to = "../src/bindings.ts")]
pub trait CmdMethods {
    async fn set_buffer<R: Runtime>(
        app_handle: AppHandle<R>,
        buffer: Buffer,
    ) -> Result<LuxBuffer, String>;
    async fn update_channel_value<R: Runtime>(
        app_handle: AppHandle<R>,
        channel_number: usize,
        value: u8,
    ) -> Result<LuxBuffer, String>;
    async fn insert_channel<R: Runtime>(
        app_handle: AppHandle<R>,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String>;
    async fn delete_channel<R: Runtime>(
        app_handle: AppHandle<R>,
        channel_number: usize,
    ) -> Result<(), String>;
    async fn update_channel_metadata<R: Runtime>(
        app_handle: AppHandle<R>,
        channel_number: usize,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String>;
    async fn sync_state<R: Runtime>(app_handle: AppHandle<R>) -> Result<String, String>;

    #[taurpc(event)]
    async fn channel_data_set(channels: [LuxChannel; BUFFER_SIZE]);
}

#[derive(Clone)]
pub struct CmdEndpoint;

#[taurpc::resolvers]
impl CmdMethods for CmdEndpoint {
    async fn set_buffer<R: Runtime>(
        self,
        app_handle: AppHandle<R>,
        buffer: Buffer,
    ) -> Result<LuxBuffer, String> {
        log::trace!("received buffer {:?}", buffer);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        state.set(buffer, app_handle.clone())
    }

    async fn update_channel_value<R: Runtime>(
        self,
        app_handle: AppHandle<R>,
        channel_number: usize,
        value: u8,
    ) -> Result<LuxBuffer, String> {
        log::debug!("received channel {} to {}", channel_number, value);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        state.set_channel(channel_number, value, app_handle.clone())
    }

    async fn insert_channel<R: Runtime>(
        self,
        app_handle: AppHandle<R>,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        log::trace!("received channel {:?}", new_metadata);
        let mut state = app_handle.state::<LuxChannels>().inner().clone();
        state.set(
            new_metadata.get_channel_number(),
            new_metadata,
            app_handle.clone(),
        )
    }

    async fn delete_channel<R: Runtime>(
        self,
        _app_handle: AppHandle<R>,
        channel_number: usize,
    ) -> Result<(), String> {
        log::debug!("received channel {} to delete", channel_number);
        Ok(())
    }

    async fn update_channel_metadata<R: Runtime>(
        self,
        app_handle: AppHandle<R>,
        channel_number: usize,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        log::trace!("received channel {:?}", new_metadata);
        let mut state = app_handle.state::<LuxChannels>().inner().clone();
        state.set(channel_number, new_metadata, app_handle.clone())
    }

    async fn sync_state<R: Runtime>(self, app: AppHandle<R>) -> Result<String, String> {
        log::trace!("sync_state");
        SyncEndpoint.sync_state(app.clone()).await
    }
}
