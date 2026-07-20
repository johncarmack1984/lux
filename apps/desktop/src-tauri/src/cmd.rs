use crate::guest::SharedSetup;
use crate::lock::LockPolicy;
use crate::{
    account::{AuthStatus, LuxAccount},
    buffer::{Buffer, LuxBuffer, UNIVERSE_SIZE},
    channel::LuxChannel,
    channels::LuxChannels,
    devices::{self, DmxDeviceInfo, DmxOutput},
    fixture::{self, ChannelDef, Fixture, FixturePreset},
    nudge::RemotePeer,
    settings::{SliderOrientation, UserSettings},
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
    fn delete_setup(&self, app_handle: AppHandle, id: String) -> Result<Vec<SetupSummary>, String>;
    fn set_active_setup(&self, app_handle: AppHandle, id: String) -> Result<SetupSummary, String>;
    // Collapsed fixture cards — device-local UI state, persisted with the
    // store but never synced.
    fn get_collapsed_fixtures(&self, app_handle: AppHandle) -> Result<Vec<String>, String>;
    fn set_fixture_collapsed(
        &self,
        app_handle: AppHandle,
        id: String,
        collapsed: bool,
    ) -> Result<Vec<String>, String>;
    // User settings — persisted locally and cloud-synced when signed in.
    fn get_settings(&self, app_handle: AppHandle) -> Result<UserSettings, String>;
    fn set_slider_orientation(
        &self,
        app_handle: AppHandle,
        orientation: SliderOrientation,
    ) -> Result<UserSettings, String>;
    // Accounts — Cognito identity (gates cloud sync; no-op when COGNITO_* unset).
    fn auth_status(&self, app_handle: AppHandle) -> Result<AuthStatus, String>;
    fn sign_up(&self, app_handle: AppHandle, email: String, password: String)
        -> Result<(), String>;
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
    /// Run the native Sign in with Apple sheet and exchange its identity token
    /// for a session. Async: the sheet is user-paced and its callbacks arrive
    /// on the main thread, which must stay unblocked — a sync procedure would
    /// run (and block) exactly there. Rejects with "canceled" (verbatim) when
    /// the user dismisses the sheet.
    async fn sign_in_with_apple(&self, app_handle: AppHandle) -> Result<AuthStatus, String>;
    fn sign_out(&self, app_handle: AppHandle) -> Result<AuthStatus, String>;
    fn delete_account(&self, app_handle: AppHandle) -> Result<AuthStatus, String>;
    // Cloud sync — current status for the indicator, and a manual pull (fired on
    // window focus so remote edits land without waiting for a restart).
    fn sync_status(&self, app_handle: AppHandle) -> Result<crate::cloud::SyncState, String>;
    fn sync_now(&self, app_handle: AppHandle) -> Result<(), String>;
    // Remote control — the user's other live devices, learned from presence
    // cards on the user channel. Backend-driven, so the UI polls it (events
    // don't reliably reach the webview on iOS).
    fn list_remote_peers(&self, app_handle: AppHandle) -> Result<Vec<RemotePeer>, String>;
    // Paired headless devices (lux-node boxes) from the account's registry —
    // the Devices list, and the delete-account confirm (blast radius).
    fn list_paired_devices(&self, app_handle: AppHandle) -> Result<Vec<PairedDevice>, String>;
    // Add-device pairing: the same-egress pending list, then approve one onto a
    // setup, then remove a paired device. Sync (blocking HTTP off-thread, like
    // list_paired_devices) — the UI polls the pending list while its dialog is open.
    fn list_pending_devices(&self, app_handle: AppHandle) -> Result<Vec<PendingDevice>, String>;
    fn approve_device(
        &self,
        app_handle: AppHandle,
        pair_ref: String,
        setup_id: String,
        universe: Option<u16>,
        name: Option<String>,
    ) -> Result<(), String>;
    fn remove_device(&self, app_handle: AppHandle, device_id: String) -> Result<(), String>;
    // Shared control in both directions, for the same confirm: deleting an
    // account ends every share it is part of, including ones other people
    // depend on.
    fn list_shares(&self, app_handle: AppHandle) -> Result<ShareTally, String>;
    // Shared control, owner side: mint a code for one of my setups, see who
    // holds a grant on what, and end either a grant or an unclaimed code.
    fn invite_to_setup(
        &self,
        app_handle: AppHandle,
        setup_id: String,
        label: Option<String>,
    ) -> Result<InviteCode, String>;
    fn list_granted_shares(&self, app_handle: AppHandle) -> Result<GrantedShares, String>;
    fn revoke_share(
        &self,
        app_handle: AppHandle,
        contact_sub: String,
        setup_id: String,
    ) -> Result<(), String>;
    fn withdraw_invite(&self, app_handle: AppHandle, code_ref: String) -> Result<(), String>;
    // Shared control, guest side: setups other people have shared with this
    // account. `open_shared_desk` returns everything a surface needs to draw
    // one — the owner's compiled setup plus their applier's last-known buffer —
    // in a single call, because a guest holds no copy of either.
    fn claim_share(&self, app_handle: AppHandle, code: String) -> Result<SharedSetup, String>;
    fn list_shared_setups(&self, app_handle: AppHandle) -> Result<Vec<SharedSetup>, String>;
    fn open_shared_desk(
        &self,
        app_handle: AppHandle,
        owner_sub: String,
        setup_id: String,
    ) -> Result<Option<SharedDesk>, String>;
    fn close_shared_desk(&self, app_handle: AppHandle) -> Result<(), String>;
    // Guest control writes. These publish to the *owner's* frame topic and
    // never touch this device's own buffer — a guest moving a fader on someone
    // else's desk must not move their own fixtures.
    fn set_shared_channel(
        &self,
        app_handle: AppHandle,
        channel_number: u16,
        value: u8,
    ) -> Result<(), String>;
    fn set_shared_buffer(&self, app_handle: AppHandle, buffer: Vec<u8>) -> Result<(), String>;
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
/// One paired headless device (a lux-node box) on the account, mirrored from
/// [`lux_wire::device::DeviceRecord`] for the Devices list and the
/// delete-account confirm's tally. `paired_at` is epoch millis (an `f64` so it
/// crosses to the webview as a plain `number`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
    pub hostname: String,
    pub setup_id: String,
    pub universe: u16,
    pub paired_at: f64,
}

/// One pending (unclaimed, same-egress) device on the Add-device screen,
/// mirrored from [`lux_wire::device::PendingDevice`]. `pair_ref` is the opaque
/// handle the approve call takes; `first_seen` is epoch millis as an `f64`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingDevice {
    pub pair_ref: String,
    pub user_code: String,
    pub hostname: String,
    pub mac_tail: String,
    pub version: String,
    pub arch: String,
    pub first_seen: f64,
}

/// Who the account's shared-control grants involve, thinned from
/// [`lux_wire::shares::SharesResponse`] to the names a confirm dialog reads
/// out. One person holding two of the caller's setups is one name here — the
/// dialog counts *people*, because that is what the user is about to affect.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareTally {
    /// People who can currently control one of the caller's setups.
    pub granted_to: Vec<String>,
    /// People whose setups the caller can currently control.
    pub received_from: Vec<String>,
}

/// A freshly minted invite code. The only time the code itself exists in
/// readable form — the server stores nothing but its hash, so a code that is
/// lost is withdrawn and re-minted rather than recovered.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InviteCode {
    pub code: String,
    /// Handle for withdrawing this code before anyone claims it.
    pub code_ref: String,
    /// Epoch millis (an `f64` so it crosses to the webview as a plain number).
    pub expires_at: f64,
}

/// One contact who can control one of the caller's setups.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GrantedShare {
    pub contact_sub: String,
    /// The contact's account email, recorded when they claimed.
    pub contact_label: String,
    pub setup_id: String,
    pub setup_name: Option<String>,
    /// The owner's own private note from the invite, if they set one.
    pub label: Option<String>,
}

/// One outstanding code the caller has handed out and nobody has claimed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingShare {
    pub code_ref: String,
    pub setup_id: String,
    pub setup_name: Option<String>,
    pub label: Option<String>,
    /// Epoch millis (an `f64`; see [`InviteCode`]).
    pub expires_at: f64,
}

/// The owner's whole sharing picture: who holds a grant, and which codes are
/// still out there unclaimed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct GrantedShares {
    pub granted: Vec<GrantedShare>,
    pub pending: Vec<PendingShare>,
}

/// Everything a guest surface needs to draw one shared setup: the owner's
/// compiled setup, flattened, plus their applier's last-known buffer so the
/// faders open at the truth instead of at zero.
///
/// Returned whole rather than as separate calls because a guest has no copy of
/// any of it — there is no local store to read a second time, and a surface
/// that drew before the buffer arrived would show a dark desk that isn't.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SharedDesk {
    pub owner_sub: String,
    pub setup_id: String,
    /// The setup's live name, from the compiled config.
    pub name: String,
    pub universe: u16,
    pub channels: Vec<SharedChannel>,
    pub fixtures: Vec<SharedFixture>,
    /// The owner applier's last-applied buffer. Empty when it hasn't published
    /// one — a surface should render zeros, not refuse to draw.
    pub buffer: Vec<u8>,
}

/// One patched slot on a shared desk (`lux_wire::ctl::ConfigChannel`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SharedChannel {
    /// 1-based DMX slot.
    pub n: u16,
    pub name: String,
    /// Role name; a surface matches the ones it knows and treats the rest as a
    /// plain fader, so an unfamiliar value is never an error.
    pub role: String,
}

/// One fixture on a shared desk (`lux_wire::ctl::ConfigFixture`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SharedFixture {
    pub name: String,
    pub address: u16,
    pub count: u16,
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
    SettingsChanged {
        settings: UserSettings,
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
        let outgoing = buffer.clone();
        let result = state.set(buffer, app_handle.clone())?;
        // User input also drives the rig remotely. Publishing lives here at
        // the command layer only — apply paths never publish (loop guard).
        crate::nudge::publish_input_overlay(&app_handle, outgoing);
        Ok(result)
    }
    fn update_channel_value(
        &self,
        app_handle: AppHandle,
        channel_number: u32,
        value: u8,
    ) -> Result<LuxBuffer, String> {
        log::debug!("received channel {} to {}", channel_number, value);
        let mut state = app_handle.state::<LuxBuffer>().inner().clone();
        let result = state.set_channel(channel_number as usize, value, app_handle.clone())?;
        // set_channel validated the range, so the narrowing always fits.
        if let Ok(ch) = u16::try_from(channel_number) {
            crate::nudge::publish_input_channel(&app_handle, ch, value);
        }
        Ok(result)
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

    fn delete_setup(&self, app_handle: AppHandle, id: String) -> Result<Vec<SetupSummary>, String> {
        let id = parse_setup_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        let was_active = setups.active_id() == id;
        setups.delete(id)?;
        // Clear the retained config here rather than leaving it to the
        // reconcile: once the setup is out of the store nothing can name its
        // topic, and a guest's whole view of a setup is that frame — an
        // orphaned one would outlive the setup it describes.
        crate::nudge::clear_config(&app_handle, id);
        // Deleting the active setup reassigns active inside the store; re-sync the
        // output and UI to whatever became active, exactly like a manual switch.
        if was_active {
            activate(&app_handle, setups.inner())?;
        }
        commit_setups(&app_handle, setups.inner())
    }

    fn set_active_setup(&self, app_handle: AppHandle, id: String) -> Result<SetupSummary, String> {
        let id = parse_setup_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        setups.set_active(id)?;
        activate(&app_handle, setups.inner())?;
        commit_setups(&app_handle, setups.inner())?;
        Ok(setups.active_summary())
    }

    fn get_collapsed_fixtures(&self, app_handle: AppHandle) -> Result<Vec<String>, String> {
        Ok(app_handle
            .state::<LuxSetups>()
            .collapsed_fixture_ids()
            .iter()
            .map(uuid::Uuid::to_string)
            .collect())
    }

    fn set_fixture_collapsed(
        &self,
        app_handle: AppHandle,
        id: String,
        collapsed: bool,
    ) -> Result<Vec<String>, String> {
        let id = parse_fixture_id(&id)?;
        let setups = app_handle.state::<LuxSetups>();
        let ids = setups.set_fixture_collapsed(id, collapsed);
        // Local UI state only: persist, but nothing to emit or push.
        setup::save(&app_handle, setups.inner());
        Ok(ids.iter().map(uuid::Uuid::to_string).collect())
    }

    fn get_settings(&self, app_handle: AppHandle) -> Result<UserSettings, String> {
        Ok(app_handle.state::<LuxSetups>().settings())
    }

    fn set_slider_orientation(
        &self,
        app_handle: AppHandle,
        orientation: SliderOrientation,
    ) -> Result<UserSettings, String> {
        let setups = app_handle.state::<LuxSetups>();
        setups.set_slider_orientation(orientation);
        commit_settings(&app_handle, setups.inner())
    }

    fn auth_status(&self, app_handle: AppHandle) -> Result<AuthStatus, String> {
        Ok(app_handle.state::<LuxAccount>().status())
    }

    fn sign_up(
        &self,
        app_handle: AppHandle,
        email: String,
        password: String,
    ) -> Result<(), String> {
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

    async fn sign_in_with_apple(&self, app_handle: AppHandle) -> Result<AuthStatus, String> {
        let status = app_handle
            .state::<LuxAccount>()
            .sign_in_with_apple(&app_handle)
            .await?;
        emit_auth_changed(&app_handle, status.clone())?;
        // Same post-sign-in tail as the SRP path: pull the account's setups,
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

    fn delete_account(&self, app_handle: AppHandle) -> Result<AuthStatus, String> {
        // Revoke the Apple-side grant first (required when an Apple link
        // exists; a quiet no-op otherwise) — best-effort, never blocks the
        // deletion itself.
        app_handle.state::<LuxAccount>().revoke_apple_link();
        // Wipe the cloud data while the tokens still authenticate, then remove
        // the Cognito user; a failure at either step leaves the account intact
        // and the whole flow retryable.
        crate::cloud::wipe_account_data(&app_handle)?;
        let status = app_handle.state::<LuxAccount>().delete_account()?;
        // The local setups stay usable on this device as never-synced data.
        let setups = app_handle.state::<LuxSetups>();
        setups.reset_for_new_account();
        setup::save(&app_handle, &setups);
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

    fn list_remote_peers(&self, app_handle: AppHandle) -> Result<Vec<RemotePeer>, String> {
        Ok(app_handle.state::<crate::nudge::LuxNudge>().remote_peers())
    }

    fn list_paired_devices(&self, app_handle: AppHandle) -> Result<Vec<PairedDevice>, String> {
        Ok(app_handle
            .state::<LuxAccount>()
            .list_paired_devices()?
            .into_iter()
            .map(|d| PairedDevice {
                device_id: d.device_id,
                name: d.name,
                hostname: d.hostname,
                setup_id: d.setup_id,
                universe: d.universe,
                paired_at: d.paired_at as f64,
            })
            .collect())
    }

    fn list_pending_devices(&self, app_handle: AppHandle) -> Result<Vec<PendingDevice>, String> {
        Ok(app_handle
            .state::<LuxAccount>()
            .list_pending_devices()?
            .into_iter()
            .map(|d| PendingDevice {
                pair_ref: d.pair_ref,
                user_code: d.user_code,
                hostname: d.hostname,
                mac_tail: d.mac_tail,
                version: d.version,
                arch: d.arch,
                first_seen: d.first_seen as f64,
            })
            .collect())
    }

    fn approve_device(
        &self,
        app_handle: AppHandle,
        pair_ref: String,
        setup_id: String,
        universe: Option<u16>,
        name: Option<String>,
    ) -> Result<(), String> {
        app_handle
            .state::<LuxAccount>()
            .approve_device(pair_ref, setup_id, universe, name)
    }

    fn remove_device(&self, app_handle: AppHandle, device_id: String) -> Result<(), String> {
        app_handle.state::<LuxAccount>().revoke_device(device_id)
    }

    fn list_shares(&self, app_handle: AppHandle) -> Result<ShareTally, String> {
        let shares = crate::cloud::list_shares(&app_handle)?;
        // One person can hold several of the caller's setups; the dialog is
        // counting people, so fold by account. Sorted by sub rather than kept
        // in list order because the dedup below is adjacent-only, and a stable
        // order beats DynamoDB's for a sentence a human reads.
        let names = |mut seen: Vec<(String, String)>| -> Vec<String> {
            seen.dedup_by(|a, b| a.0 == b.0);
            seen.into_iter().map(|(_, label)| label).collect()
        };
        let label = |sub: &str, label: &str| {
            // Every pool user has an email, but a token without the claim is
            // still a valid token — name the account rather than show a blank.
            if label.is_empty() {
                format!("account {}", sub.chars().take(8).collect::<String>())
            } else {
                label.to_owned()
            }
        };
        let mut granted: Vec<(String, String)> = shares
            .granted
            .iter()
            .map(|g| {
                (
                    g.contact_sub.clone(),
                    label(&g.contact_sub, &g.contact_label),
                )
            })
            .collect();
        granted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut received: Vec<(String, String)> = shares
            .received
            .iter()
            .map(|g| (g.owner_sub.clone(), label(&g.owner_sub, &g.owner_label)))
            .collect();
        received.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(ShareTally {
            granted_to: names(granted),
            received_from: names(received),
        })
    }

    fn invite_to_setup(
        &self,
        app_handle: AppHandle,
        setup_id: String,
        label: Option<String>,
    ) -> Result<InviteCode, String> {
        let minted = crate::cloud::invite_to_setup(
            &app_handle,
            &setup_id,
            label.filter(|l| !l.trim().is_empty()),
        )?;
        // Publish the compiled setup now: a code is useless until the guest
        // has something to render, and this is the setup's first grant.
        crate::nudge::refresh_shares(&app_handle);
        Ok(InviteCode {
            code: minted.code,
            code_ref: minted.code_ref,
            expires_at: minted.expires_at as f64,
        })
    }

    fn list_granted_shares(&self, app_handle: AppHandle) -> Result<GrantedShares, String> {
        let shares = crate::cloud::list_shares(&app_handle)?;
        Ok(GrantedShares {
            granted: shares
                .granted
                .into_iter()
                .map(|g| GrantedShare {
                    contact_sub: g.contact_sub,
                    contact_label: g.contact_label,
                    setup_id: g.setup_id,
                    setup_name: g.setup_name,
                    label: g.label,
                })
                .collect(),
            pending: shares
                .pending
                .into_iter()
                .map(|p| PendingShare {
                    code_ref: p.code_ref,
                    setup_id: p.setup_id,
                    setup_name: p.setup_name,
                    label: p.label,
                    expires_at: p.expires_at as f64,
                })
                .collect(),
        })
    }

    fn revoke_share(
        &self,
        app_handle: AppHandle,
        contact_sub: String,
        setup_id: String,
    ) -> Result<(), String> {
        crate::cloud::end_share(
            &app_handle,
            &format!(
                "{}/{contact_sub}/{setup_id}",
                lux_wire::shares::GRANTED_SEGMENT
            ),
        )?;
        // Revoking the last grant on a setup clears its retained config, so the
        // contact's surface goes dark rather than holding a stale desk.
        crate::nudge::refresh_shares(&app_handle);
        Ok(())
    }

    fn withdraw_invite(&self, app_handle: AppHandle, code_ref: String) -> Result<(), String> {
        crate::cloud::end_share(
            &app_handle,
            &format!("{}/{code_ref}", lux_wire::shares::INVITE_SEGMENT),
        )
    }

    fn claim_share(&self, app_handle: AppHandle, code: String) -> Result<SharedSetup, String> {
        let claimed = crate::cloud::claim_share(&app_handle, &code)?;
        // Pull straight away rather than waiting on the nudge: the claimer is
        // watching this happen, and the grant is what makes the retained
        // topics readable at all.
        crate::nudge::refresh_shares(&app_handle);
        Ok(SharedSetup {
            owner_sub: claimed.owner_sub,
            owner_label: claimed.owner_label,
            setup_id: claimed.setup_id,
            setup_name: claimed.setup_name,
            // The owner's compiled setup arrives on its retained topic a moment
            // after the subscribe; the list shows the row meanwhile.
            renderable: false,
        })
    }

    fn list_shared_setups(&self, app_handle: AppHandle) -> Result<Vec<SharedSetup>, String> {
        Ok(app_handle.state::<crate::guest::LuxGuest>().shared_setups())
    }

    fn open_shared_desk(
        &self,
        app_handle: AppHandle,
        owner_sub: String,
        setup_id: String,
    ) -> Result<Option<SharedDesk>, String> {
        let guest = app_handle.state::<crate::guest::LuxGuest>();
        let Some(config) = guest.config(&owner_sub, &setup_id) else {
            // No compiled setup yet: the owner's applier may simply not be
            // running. The surface says so rather than drawing an empty desk.
            return Ok(None);
        };
        let desk = Ok(Some(SharedDesk {
            owner_sub: owner_sub.clone(),
            setup_id: setup_id.clone(),
            name: config.name,
            universe: config.universe,
            channels: config
                .channels
                .into_iter()
                .map(|c| SharedChannel {
                    n: c.n,
                    name: c.name,
                    role: c.role,
                })
                .collect(),
            fixtures: config
                .fixtures
                .into_iter()
                .map(|f| SharedFixture {
                    name: f.name,
                    address: f.address,
                    count: f.count,
                })
                .collect(),
            buffer: guest.state(&owner_sub, &setup_id).unwrap_or_default(),
        }));
        // Bind the publish target and announce this guest on the owner's desk
        // only once there is something to draw — a surface that couldn't render
        // shouldn't claim to be live on it.
        crate::guest::open_desk(&app_handle, &owner_sub, &setup_id);
        desk
    }

    fn close_shared_desk(&self, app_handle: AppHandle) -> Result<(), String> {
        crate::guest::close_desk(&app_handle);
        Ok(())
    }

    fn set_shared_channel(
        &self,
        app_handle: AppHandle,
        channel_number: u16,
        value: u8,
    ) -> Result<(), String> {
        crate::guest::publish_channel(&app_handle, channel_number, value);
        Ok(())
    }

    fn set_shared_buffer(&self, app_handle: AppHandle, buffer: Vec<u8>) -> Result<(), String> {
        crate::guest::publish_overlay(&app_handle, buffer);
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
    // A shared setup's guests render from its retained config, so it has to
    // follow the setup rather than lag it (coalesced; see refresh_shares).
    crate::nudge::refresh_shares(app);
    Ok(fixtures)
}

/// Persist the store and broadcast the new user settings to the UI.
fn commit_settings(app: &AppHandle, setups: &LuxSetups) -> Result<UserSettings, String> {
    setup::save(app, setups);
    let settings = setups.settings();
    CmdEvent::SettingsChanged { settings }
        .emit(app)
        .map_err(|e| format!("Failed to emit settings_changed event: {e}"))?;
    crate::cloud::schedule_push(app);
    Ok(settings)
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
    // A shared setup's guests render from its retained config, so it has to
    // follow the setup rather than lag it (coalesced; see refresh_shares).
    crate::nudge::refresh_shares(app);
    Ok(summaries)
}

/// Persist and broadcast the full setup state after a cloud pull changed it, and
/// retarget the live output at the (possibly changed) active universe. Called by
/// [`crate::cloud`] after reconciling a pull into the local store.
pub fn broadcast_synced_state(app: &AppHandle) {
    let setups = app.state::<LuxSetups>();
    setup::save(app, &setups);
    // A pull can change a shared setup's patch, name, or universe from another
    // device; guests render from the retained config, so it follows the pull.
    // (Coalesced, so this and the nudge that scheduled the pull are one.)
    crate::nudge::refresh_shares(app);
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
    let _ = CmdEvent::SettingsChanged {
        settings: setups.settings(),
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
    // Remote surfaces learn the new binding through the presence card.
    crate::nudge::presence_changed(app);
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
