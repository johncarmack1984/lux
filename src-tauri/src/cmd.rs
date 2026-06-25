use crate::{
    buffer::{Buffer, LuxBuffer},
    channel::LuxChannel,
    channels::LuxChannels,
    fixture::{self, ChannelDef, Fixture, FixturePreset, LuxPatch},
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
    fn list_presets(&self) -> Result<Vec<FixturePreset>, String>;
    fn get_patch(&self, app_handle: AppHandle) -> Result<Vec<Fixture>, String>;
    fn add_fixture(
        &self,
        app_handle: AppHandle,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Vec<Fixture>, String>;
    fn update_fixture(
        &self,
        app_handle: AppHandle,
        id: String,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Vec<Fixture>, String>;
    fn remove_fixture(&self, app_handle: AppHandle, id: String) -> Result<Vec<Fixture>, String>;
}
#[derive(ttipc::Event)]
pub enum CmdEvent {
    ChannelDataSet { channels: Vec<LuxChannel> },
    PatchSet { fixtures: Vec<Fixture> },
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

    fn list_presets(&self) -> Result<Vec<FixturePreset>, String> {
        Ok(fixture::presets())
    }

    fn get_patch(&self, app_handle: AppHandle) -> Result<Vec<Fixture>, String> {
        Ok(app_handle.state::<LuxPatch>().list())
    }

    fn add_fixture(
        &self,
        app_handle: AppHandle,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Vec<Fixture>, String> {
        let patch = app_handle.state::<LuxPatch>();
        patch.add(name, address, channels)?;
        commit_patch(&app_handle, patch.inner())
    }

    fn update_fixture(
        &self,
        app_handle: AppHandle,
        id: String,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Vec<Fixture>, String> {
        let id = uuid::Uuid::parse_str(&id).map_err(|e| format!("bad fixture id: {e}"))?;
        let patch = app_handle.state::<LuxPatch>();
        patch.update(id, name, address, channels)?;
        commit_patch(&app_handle, patch.inner())
    }

    fn remove_fixture(&self, app_handle: AppHandle, id: String) -> Result<Vec<Fixture>, String> {
        let id = uuid::Uuid::parse_str(&id).map_err(|e| format!("bad fixture id: {e}"))?;
        let patch = app_handle.state::<LuxPatch>();
        patch.remove(id)?;
        commit_patch(&app_handle, patch.inner())
    }
}

/// Persist the patch and broadcast it to the UI, returning the new fixture list.
fn commit_patch(app: &AppHandle, patch: &LuxPatch) -> Result<Vec<Fixture>, String> {
    fixture::save(app, patch);
    let fixtures = patch.list();
    CmdEvent::PatchSet {
        fixtures: fixtures.clone(),
    }
    .emit(app)
    .map_err(|e| format!("Failed to emit patch_set event: {e}"))?;
    Ok(fixtures)
}
