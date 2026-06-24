use crate::{
    buffer::{Buffer, LuxBuffer},
    channel::LuxChannel,
    channels::LuxChannels,
    sync::*,
};
use tauri::{AppHandle, Manager};

#[ttipc::procedures(path = "cmd")]
pub trait CmdMethods {
    fn set_buffer(&self, app_handle: AppHandle, buffer: Buffer) -> Result<LuxBuffer, String>;
    fn update_channel_value(
        &self,
        app_handle: AppHandle,
        channel_number: u32,
        value: u8,
    ) -> Result<LuxBuffer, String>;
    fn insert_channel(
        &self,
        app_handle: AppHandle,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String>;
    fn update_channel_metadata(
        &self,
        app_handle: AppHandle,
        channel_number: u32,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String>;
    fn sync_state(&self, app_handle: AppHandle) -> Result<String, String>;
}
#[derive(ttipc::Event)]
pub enum CmdEvent {
    ChannelDataSet { channels: Vec<LuxChannel> },
}

#[derive(Clone)]
pub struct CmdEndpoint;

impl CmdMethods for CmdEndpoint {
    fn set_buffer(&self, app_handle: AppHandle, buffer: Buffer) -> Result<LuxBuffer, String> {
        log::trace!("received buffer {:?}", buffer);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        state.set(buffer, app_handle.clone())
    }
    fn update_channel_value(
        &self,
        app_handle: AppHandle,
        channel_number: u32,
        value: u8,
    ) -> Result<LuxBuffer, String> {
        log::debug!("received channel {} to {}", channel_number, value);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        state.set_channel(channel_number as usize, value, app_handle.clone())
    }
    fn insert_channel(
        &self,
        app_handle: AppHandle,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        log::trace!("received channel {:?}", new_metadata);
        let mut state = app_handle.state::<LuxChannels>().inner().clone();
        state.set(
            new_metadata.get_channel_number() as usize,
            new_metadata,
            app_handle.clone(),
        )
    }
    fn update_channel_metadata(
        &self,
        app_handle: AppHandle,
        channel_number: u32,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        log::trace!("received channel {:?}", new_metadata);
        let mut state = app_handle.state::<LuxChannels>().inner().clone();
        state.set(channel_number as usize, new_metadata, app_handle.clone())
    }
    fn sync_state(&self, app: AppHandle) -> Result<String, String> {
        log::trace!("sync_state");
        SyncEndpoint.sync_state(app.clone())
    }
}
