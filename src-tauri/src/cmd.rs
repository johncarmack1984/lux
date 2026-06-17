// Migrated from TauRPC by ttipc-migrate. Manual follow-ups:
//   - errors: each `Result<_, E>` needs `E: ttipc::Error` (derive it on the error type).
//   - imports: drop the now-unused `Runtime`; `Channel` is now `ttipc::Channel`.
//   - de-async: methods with no blocking `.await` were made sync (ttipc's default), and their `.await`s on now-sync siblings were dropped.
//   - async injection: ttipc rejects an `async` procedure that takes `AppHandle`/`State<T>`; make it sync, or rework so the async body needs no injected handle/state.
//   - events: `#[taurpc(event)]` methods were lifted into a `#[derive(ttipc::Event)]` enum (matching emit sites were rewritten to `Enum::Variant.emit(&h)`); drop any now-empty trait/impl.

use crate::{
    buffer::{Buffer, LuxBuffer, BUFFER_SIZE},
    channel::LuxChannel,
    channels::LuxChannels,
    sync::*,
};
use tauri::{AppHandle, Manager, Runtime};

#[ttipc::procedures(path = "cmd")]
pub trait CmdMethods {
    fn set_buffer(
        &self,
        app_handle: AppHandle,
        buffer: Buffer,
    ) -> Result<LuxBuffer, String>;
    fn update_channel_value(
        &self,
        app_handle: AppHandle,
        channel_number: usize,
        value: u8,
    ) -> Result<LuxBuffer, String>;
    fn insert_channel(
        &self,
        app_handle: AppHandle,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String>;
    fn delete_channel(
        &self,
        app_handle: AppHandle,
        channel_number: usize,
    ) -> Result<(), String>;
    fn update_channel_metadata(
        &self,
        app_handle: AppHandle,
        channel_number: usize,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String>;
    async fn sync_state(&self, app_handle: AppHandle) -> Result<String, String>;
}
#[derive(ttipc::Event)]
pub enum CmdEvent {
    ChannelDataSet { channels: [LuxChannel; BUFFER_SIZE] },
}

#[derive(Clone)]
pub struct CmdEndpoint;

impl CmdMethods for CmdEndpoint {
    fn set_buffer(
        &self,
        app_handle: AppHandle,
        buffer: Buffer,
    ) -> Result<LuxBuffer, String> {
        log::trace!("received buffer {:?}", buffer);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        state.set(buffer, app_handle.clone())
    }
    fn update_channel_value(
        &self,
        app_handle: AppHandle,
        channel_number: usize,
        value: u8,
    ) -> Result<LuxBuffer, String> {
        log::debug!("received channel {} to {}", channel_number, value);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        state.set_channel(channel_number, value, app_handle.clone())
    }
    fn insert_channel(
        &self,
        app_handle: AppHandle,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        log::trace!("received channel {:?}", new_metadata);
        let mut state = app_handle.state::<LuxChannels>().inner().clone();
        state.set(new_metadata.get_channel_number(), new_metadata, app_handle.clone())
    }
    fn delete_channel(
        &self,
        _app_handle: AppHandle,
        channel_number: usize,
    ) -> Result<(), String> {
        log::debug!("received channel {} to delete", channel_number);
        Ok(())
    }
    fn update_channel_metadata(
        &self,
        app_handle: AppHandle,
        channel_number: usize,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        log::trace!("received channel {:?}", new_metadata);
        let mut state = app_handle.state::<LuxChannels>().inner().clone();
        state.set(channel_number, new_metadata, app_handle.clone())
    }
    async fn sync_state(&self, app: AppHandle) -> Result<String, String> {
        log::trace!("sync_state");
        SyncEndpoint.sync_state(app.clone()).await
    }
}
