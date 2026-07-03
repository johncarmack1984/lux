use crate::lock::LockPolicy;
use crate::{
    account::{AuthStatus, LuxAccount},
    buffer::{Buffer, LuxBuffer, UNIVERSE_SIZE},
    channel::LuxChannel,
    channels::LuxChannels,
    devices::{self, DmxDeviceInfo, DmxOutput},
    fixture::{self, ChannelDef, Fixture, FixturePreset},
    setup::{self, LuxSetups, SetupSummary},
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
    // Fixtures — operate on the active setup's patch.
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
    // Setups — a user's named (fixtures + universe) configurations.
    fn list_setups(&self, app_handle: AppHandle) -> Result<Vec<SetupSummary>, String>;
    fn create_setup(
        &self,
        app_handle: AppHandle,
        name: String,
        universe: u16,
    ) -> Result<Vec<SetupSummary>, String>;
    fn rename_setup(
        &self,
        app_handle: AppHandle,
        id: String,
        name: String,
    ) -> Result<Vec<SetupSummary>, String>;
    fn set_setup_universe(
        &self,
        app_handle: AppHandle,
        id: String,
        universe: u16,
    ) -> Result<Vec<SetupSummary>, String>;
    fn delete_setup(&self, app_handle: AppHandle, id: String)
        -> Result<Vec<SetupSummary>, String>;
    fn set_active_setup(&self, app_handle: AppHandle, id: String) -> Result<SetupSummary, String>;
    // Accounts — Cognito identity (gates cloud sync; no-op when COGNITO_* unset).
    fn auth_status(&self, app_handle: AppHandle) -> Result<AuthStatus, String>;
    fn sign_up(&self, app_handle: AppHandle, email: String, password: String) -> Result<(), String>;
    fn confirm_sign_up(
        &self,
        app_handle: AppHandle,
        email: String,
        code: String,
    ) -> Result<(), String>;
    fn sign_in(
        &self,
        app_handle: AppHandle,
        email: String,
        password: String,
    ) -> Result<AuthStatus, String>;
    fn sign_out(&self, app_handle: AppHandle) -> Result<AuthStatus, String>;
    // Cloud sync — current status for the indicator, and a manual pull (fired on
    // window focus so remote edits land without waiting for a restart).
    fn sync_status(&self, app_handle: AppHandle) -> Result<crate::cloud::SyncState, String>;
    fn sync_now(&self, app_handle: AppHandle) -> Result<(), String>;
    // DMX output — the in-app device picker (the only output selector on mobile,
    // where there's no tray). Mirrors the desktop tray's device menu.
    fn list_dmx_devices(&self, app_handle: AppHandle) -> Result<Vec<DmxDeviceInfo>, String>;
    fn set_dmx_device(
        &self,
        app_handle: AppHandle,
        key: String,
    ) -> Result<Vec<DmxDeviceInfo>, String>;
    fn rescan_dmx_devices(&self, app_handle: AppHandle) -> Result<(), String>;
}
#[derive(ttipc::Event)]
pub enum CmdEvent {
    ChannelDataSet {
        channels: Vec<LuxChannel>,
    },
    PatchSet {
        setup_id: String,
        fixtures: Vec<Fixture>,
    },
    SetupsChanged {
        setups: Vec<SetupSummary>,
        active_setup_id: String,
    },
    AuthChanged {
        status: AuthStatus,
    },
    SyncStatusChanged {
        state: crate::cloud::SyncState,
    },
    DmxDevicesChanged {
        devices: Vec<DmxDeviceInfo>,
    },
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
        Ok(app_handle.state::<LuxSetups>().active_fixtures())
    }

    fn add_fixture(
        &self,
        app_handle: AppHandle,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Vec<Fixture>, String> {
        let setups = app_handle.state::<LuxSetups>();
        setups.add_fixture(name, address, channels)?;
        commit_patch(&app_handle, setups.inner())
    }

    fn update_fixture(
        &self,
        app_handle: AppHandle,
        id: String,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<Vec<Fixture>, String> {
        let id = parse_fixture_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        setups.update_fixture(id, name, address, channels)?;
        commit_patch(&app_handle, setups.inner())
    }

    fn remove_fixture(&self, app_handle: AppHandle, id: String) -> Result<Vec<Fixture>, String> {
        let id = parse_fixture_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        setups.remove_fixture(id)?;
        commit_patch(&app_handle, setups.inner())
    }

    fn list_setups(&self, app_handle: AppHandle) -> Result<Vec<SetupSummary>, String> {
        Ok(app_handle.state::<LuxSetups>().summaries())
    }

    fn create_setup(
        &self,
        app_handle: AppHandle,
        name: String,
        universe: u16,
    ) -> Result<Vec<SetupSummary>, String> {
        let setups = app_handle.state::<LuxSetups>();
        setups.create(name, universe)?;
        commit_setups(&app_handle, setups.inner())
    }

    fn rename_setup(
        &self,
        app_handle: AppHandle,
        id: String,
        name: String,
    ) -> Result<Vec<SetupSummary>, String> {
        let id = parse_setup_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        setups.rename(id, name)?;
        commit_setups(&app_handle, setups.inner())
    }

    fn set_setup_universe(
        &self,
        app_handle: AppHandle,
        id: String,
        universe: u16,
    ) -> Result<Vec<SetupSummary>, String> {
        let id = parse_setup_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        setups.set_universe(id, universe)?;
        // Retuning the *active* setup's universe takes effect on the wire now;
        // a non-active setup just stores the value for when it's next activated.
        if setups.active_id() == id {
            devices::set_active_universe(&app_handle, setups.active_universe());
            rerender_current(&app_handle);
        }
        commit_setups(&app_handle, setups.inner())
    }

    fn delete_setup(
        &self,
        app_handle: AppHandle,
        id: String,
    ) -> Result<Vec<SetupSummary>, String> {
        let id = parse_setup_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        let was_active = setups.active_id() == id;
        setups.delete(id)?;
        // Deleting the active setup reassigns active inside the store; re-sync the
        // output and UI to whatever became active, exactly like a manual switch.
        if was_active {
            activate(&app_handle, setups.inner())?;
        }
        commit_setups(&app_handle, setups.inner())
    }

    fn set_active_setup(
        &self,
        app_handle: AppHandle,
        id: String,
    ) -> Result<SetupSummary, String> {
        let id = parse_setup_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        setups.set_active(id)?;
        activate(&app_handle, setups.inner())?;
        commit_setups(&app_handle, setups.inner())?;
        Ok(setups.active_summary())
    }

    fn auth_status(&self, app_handle: AppHandle) -> Result<AuthStatus, String> {
        Ok(app_handle.state::<LuxAccount>().status())
    }

    fn sign_up(&self, app_handle: AppHandle, email: String, password: String) -> Result<(), String> {
        app_handle.state::<LuxAccount>().sign_up(email, password)
    }

    fn confirm_sign_up(
        &self,
        app_handle: AppHandle,
        email: String,
        code: String,
    ) -> Result<(), String> {
        app_handle
            .state::<LuxAccount>()
            .confirm_sign_up(email, code)
    }

    fn sign_in(
        &self,
        app_handle: AppHandle,
        email: String,
        password: String,
    ) -> Result<AuthStatus, String> {
        let status = app_handle.state::<LuxAccount>().sign_in(email, password)?;
        emit_auth_changed(&app_handle, status.clone())?;
        // Pull the account's setups (claiming local ones on first sign-in),
        // then listen for change nudges from their other devices.
        crate::cloud::schedule_sync(&app_handle);
        crate::nudge::start(&app_handle);
        Ok(status)
    }

    fn sign_out(&self, app_handle: AppHandle) -> Result<AuthStatus, String> {
        let status = app_handle.state::<LuxAccount>().sign_out();
        crate::nudge::stop(&app_handle);
        emit_auth_changed(&app_handle, status.clone())?;
        Ok(status)
    }

    fn sync_status(&self, app_handle: AppHandle) -> Result<crate::cloud::SyncState, String> {
        Ok(app_handle.state::<crate::cloud::LuxSync>().snapshot())
    }

    fn sync_now(&self, app_handle: AppHandle) -> Result<(), String> {
        crate::cloud::schedule_sync(&app_handle);
        Ok(())
    }

    fn list_dmx_devices(&self, app_handle: AppHandle) -> Result<Vec<DmxDeviceInfo>, String> {
        Ok(devices::device_list(&app_handle))
    }

    fn set_dmx_device(
        &self,
        app_handle: AppHandle,
        key: String,
    ) -> Result<Vec<DmxDeviceInfo>, String> {
        devices::select_device(&app_handle, &key)?;
        emit_dmx_devices_changed(&app_handle);
        Ok(devices::device_list(&app_handle))
    }

    fn rescan_dmx_devices(&self, app_handle: AppHandle) -> Result<(), String> {
        // Spawns a detection pass; the refreshed list arrives via DmxDevicesChanged.
        devices::rescan(&app_handle);
        Ok(())
    }
}

fn parse_fixture_id(id: &str) -> Result<uuid::Uuid, String> {
    uuid::Uuid::parse_str(id).map_err(|e| format!("bad fixture id: {e}"))
}

fn parse_setup_id(id: &str) -> Result<uuid::Uuid, String> {
    uuid::Uuid::parse_str(id).map_err(|e| format!("bad setup id: {e}"))
}

/// Persist the store and broadcast the active setup's patch to the UI, returning
/// the new fixture list. The `setup_id` lets the UI ignore a `PatchSet` from a
/// setup it has already switched away from.
fn commit_patch(app: &AppHandle, setups: &LuxSetups) -> Result<Vec<Fixture>, String> {
    setup::save(app, setups);
    let fixtures = setups.active_fixtures();
    CmdEvent::PatchSet {
        setup_id: setups.active_id().to_string(),
        fixtures: fixtures.clone(),
    }
    .emit(app)
    .map_err(|e| format!("Failed to emit patch_set event: {e}"))?;
    crate::cloud::schedule_push(app);
    Ok(fixtures)
}

/// Persist the store and broadcast the setup list + active id to the UI.
fn commit_setups(app: &AppHandle, setups: &LuxSetups) -> Result<Vec<SetupSummary>, String> {
    setup::save(app, setups);
    let summaries = setups.summaries();
    CmdEvent::SetupsChanged {
        setups: summaries.clone(),
        active_setup_id: setups.active_id().to_string(),
    }
    .emit(app)
    .map_err(|e| format!("Failed to emit setups_changed event: {e}"))?;
    crate::cloud::schedule_push(app);
    Ok(summaries)
}

/// Persist and broadcast the full setup state after a cloud pull changed it, and
/// retarget the live output at the (possibly changed) active universe. Called by
/// [`crate::cloud`] after reconciling a pull into the local store.
pub fn broadcast_synced_state(app: &AppHandle) {
    let setups = app.state::<LuxSetups>();
    setup::save(app, &setups);
    devices::set_active_universe(app, setups.active_universe());
    let active_id = setups.active_id().to_string();
    let _ = CmdEvent::SetupsChanged {
        setups: setups.summaries(),
        active_setup_id: active_id.clone(),
    }
    .emit(app);
    let _ = CmdEvent::PatchSet {
        setup_id: active_id,
        fixtures: setups.active_fixtures(),
    }
    .emit(app);
}

/// Broadcast the new auth status so the nav/account UI updates reactively (also
/// fired on a silent session restore at startup).
fn emit_auth_changed(app: &AppHandle, status: AuthStatus) -> Result<(), String> {
    CmdEvent::AuthChanged { status }
        .emit(app)
        .map_err(|e| format!("Failed to emit auth_changed event: {e}"))
}

/// Build the current DMX device list and broadcast it so the in-app output
/// picker reflects detection/selection changes. Generic over the runtime: the
/// auto-detect path calls it from [`crate::devices::apply_detection`].
pub fn emit_dmx_devices_changed<R: tauri::Runtime>(app: &AppHandle<R>) {
    let _ = CmdEvent::DmxDevicesChanged {
        devices: devices::device_list(app),
    }
    .emit(app);
}

/// Apply the consequences of the active setup changing: point the live sACN
/// output at the new setup's universe, black out the universe so one setup's
/// levels never bleed onto another's fixtures, and re-broadcast the new patch.
fn activate(app: &AppHandle, setups: &LuxSetups) -> Result<(), String> {
    devices::set_active_universe(app, setups.active_universe());
    let mut buffer = app.state::<LuxBuffer>().inner().clone();
    buffer.set(vec![0u8; UNIVERSE_SIZE], app.clone())?; // emits BufferSet + renders
    CmdEvent::PatchSet {
        setup_id: setups.active_id().to_string(),
        fixtures: setups.active_fixtures(),
    }
    .emit(app)
    .map_err(|e| format!("Failed to emit patch_set event: {e}"))?;
    Ok(())
}

/// Re-send the current buffer to the active output without touching the UI —
/// used after the live universe number changes so the new universe lights up
/// with the levels already on screen.
fn rerender_current(app: &AppHandle) {
    let buffer = app.state::<LuxBuffer>().buffer.lock_or_recover().clone();
    if let Err(e) = app.state::<DmxOutput>().render(&buffer) {
        log::trace!("post-universe-change render failed: {e}");
    }
}
