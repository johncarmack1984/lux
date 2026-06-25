//! Named Setups: a user's lighting configurations, each a fixture patch bound to
//! one DMX universe.
//!
//! This generalizes the former single anonymous patch (`patch.json`, a bare
//! `Vec<Fixture>`) into many named [`Setup`]s wrapped in a versioned
//! [`SetupStore`] persisted to `app_config_dir()/setups.json`. A user owns
//! several setups ("Home", "Church", "Work"); exactly one is active at a time,
//! and the active setup's fixtures are what the fixture commands read and write.
//!
//! Identity is captured now — a local `user_id` plus per-setup ids — so cloud
//! accounts can adopt these setups later without re-keying. On first launch with
//! a legacy `patch.json`, [`load`] migrates it into a single "Home" setup and
//! leaves the old file behind as `patch.json.migrated`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime};

use crate::fixture::{self, ChannelDef, Fixture};

/// Current on-disk schema version for `setups.json`. Bump when the shape changes
/// and add a step to [`migrate_version`].
const STORE_VERSION: u32 = 1;

/// Default sACN/E1.31 universe for a new setup.
const DEFAULT_UNIVERSE: u16 = 1;

/// A named lighting configuration: a fixture patch bound to one DMX universe.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    #[specta(type = String)]
    pub id: uuid::Uuid,
    pub name: String,
    /// sACN/E1.31 universe this setup transmits on (1..=63999).
    pub universe: u16,
    pub fixtures: Vec<Fixture>,
    /// Server timestamp (epoch millis) from the last successful push or pull —
    /// the optimistic-concurrency base for the next push. `None` until first
    /// synced (so the cloud layer treats it as a create). Cloud-sync metadata;
    /// defaults keep older `setups.json` readable.
    #[serde(default)]
    pub updated_at: Option<i64>,
    /// Local edits not yet pushed to the cloud.
    #[serde(default)]
    pub dirty: bool,
}

impl Setup {
    /// Whether this setup has changes the cloud doesn't have yet: explicit local
    /// edits, or a setup that has never been synced.
    fn needs_push(&self) -> bool {
        self.dirty || self.updated_at.is_none()
    }
}

/// The whole persisted store (`app_config_dir()/setups.json`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupStore {
    pub version: u32,
    /// Local placeholder identity until cloud accounts land; lets these setups
    /// bind to a real user later without re-keying.
    #[specta(type = String)]
    pub user_id: uuid::Uuid,
    #[specta(type = String)]
    pub active_setup_id: uuid::Uuid,
    pub setups: Vec<Setup>,
    /// The signed-in account (email) this store is synced with, once claimed.
    /// Lets us notice a *different* account signing in on the same device and
    /// avoid leaking one user's setups into another's. `None` until first claim.
    #[serde(default)]
    pub bound_email: Option<String>,
    /// Setups deleted locally that still need a tombstone pushed to the cloud so
    /// the delete propagates to other devices. Drained as each push succeeds.
    #[serde(default)]
    pub pending_deletes: Vec<PendingDelete>,
}

/// A local delete awaiting a cloud tombstone.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingDelete {
    #[specta(type = String)]
    pub setup_id: uuid::Uuid,
    /// The setup's last-known server timestamp, as the tombstone's concurrency base.
    pub base_updated_at: Option<i64>,
}

/// A lightweight setup descriptor for the switcher UI — no fixtures, so listing
/// setups doesn't ship every patch.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupSummary {
    #[specta(type = String)]
    pub id: uuid::Uuid,
    pub name: String,
    pub universe: u16,
    pub fixture_count: u32,
    pub active: bool,
}

fn new_setup(name: impl Into<String>, universe: u16, fixtures: Vec<Fixture>) -> Setup {
    Setup {
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        universe: normalize_universe(universe),
        fixtures,
        updated_at: None,
        dirty: false,
    }
}

/// sACN universes are 1..=63999; clamp anything out of range so the wire layer
/// always gets a valid number.
fn normalize_universe(universe: u16) -> u16 {
    universe.clamp(1, 63999)
}

impl Default for SetupStore {
    fn default() -> Self {
        // First run reproduces the original single RGBAW@1 patch, as one setup.
        SetupStore::single(new_setup(
            "Home",
            DEFAULT_UNIVERSE,
            fixture::default_fixtures(),
        ))
    }
}

impl SetupStore {
    fn single(setup: Setup) -> Self {
        SetupStore {
            version: STORE_VERSION,
            user_id: uuid::Uuid::new_v4(),
            active_setup_id: setup.id,
            setups: vec![setup],
            bound_email: None,
            pending_deletes: Vec::new(),
        }
    }

    fn summaries(&self) -> Vec<SetupSummary> {
        self.setups
            .iter()
            .map(|s| SetupSummary {
                id: s.id,
                name: s.name.clone(),
                universe: s.universe,
                fixture_count: s.fixtures.len() as u32,
                active: s.id == self.active_setup_id,
            })
            .collect()
    }

    /// The active setup. `reconcile` guarantees `active_setup_id` resolves and
    /// the store is non-empty, so this never panics in practice; the fallback to
    /// the first setup keeps it total even if those invariants were skipped.
    fn active(&self) -> &Setup {
        self.setups
            .iter()
            .find(|s| s.id == self.active_setup_id)
            .or_else(|| self.setups.first())
            .expect("a store always holds at least one setup")
    }

    fn active_mut(&mut self) -> &mut Setup {
        let id = self.active_setup_id;
        let idx = self
            .setups
            .iter()
            .position(|s| s.id == id)
            .unwrap_or(0);
        &mut self.setups[idx]
    }
}

// --- managed state ----------------------------------------------------------

/// Tauri-managed store of the user's setups. All mutations go through here; the
/// caller (`cmd.rs`) persists with [`save`] and re-emits to the UI afterward.
#[derive(Debug)]
pub struct LuxSetups {
    pub store: Arc<Mutex<SetupStore>>,
}

impl From<SetupStore> for LuxSetups {
    fn from(store: SetupStore) -> Self {
        LuxSetups {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

impl LuxSetups {
    pub fn summaries(&self) -> Vec<SetupSummary> {
        self.store.lock().unwrap().summaries()
    }

    pub fn active_id(&self) -> uuid::Uuid {
        self.store.lock().unwrap().active_setup_id
    }

    pub fn active_universe(&self) -> u16 {
        self.store.lock().unwrap().active().universe
    }

    pub fn active_fixtures(&self) -> Vec<Fixture> {
        self.store.lock().unwrap().active().fixtures.clone()
    }

    pub fn active_summary(&self) -> SetupSummary {
        let store = self.store.lock().unwrap();
        let active = store.active();
        SetupSummary {
            id: active.id,
            name: active.name.clone(),
            universe: active.universe,
            fixture_count: active.fixtures.len() as u32,
            active: true,
        }
    }

    /// Snapshot of the whole store, for persistence.
    pub fn snapshot(&self) -> SetupStore {
        self.store.lock().unwrap().clone()
    }

    // -- fixture ops on the active setup --

    pub fn add_fixture(
        &self,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let active = store.active_mut();
        fixture::add(&mut active.fixtures, name, address, channels)?;
        active.dirty = true;
        Ok(())
    }

    pub fn update_fixture(
        &self,
        id: uuid::Uuid,
        name: String,
        address: u16,
        channels: Vec<ChannelDef>,
    ) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let active = store.active_mut();
        fixture::update(&mut active.fixtures, id, name, address, channels)?;
        active.dirty = true;
        Ok(())
    }

    pub fn remove_fixture(&self, id: uuid::Uuid) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let active = store.active_mut();
        fixture::remove(&mut active.fixtures, id)?;
        active.dirty = true;
        Ok(())
    }

    // -- setup CRUD --

    /// Create a new (empty) setup and return its id. Does not switch to it.
    pub fn create(&self, name: String, universe: u16) -> Result<uuid::Uuid, String> {
        let name = clean_name(name)?;
        let setup = new_setup(name, universe, Vec::new());
        let id = setup.id;
        self.store.lock().unwrap().setups.push(setup);
        Ok(id)
    }

    pub fn rename(&self, id: uuid::Uuid, name: String) -> Result<(), String> {
        let name = clean_name(name)?;
        let mut store = self.store.lock().unwrap();
        let setup = store
            .setups
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("setup {id} not found"))?;
        setup.name = name;
        setup.dirty = true;
        Ok(())
    }

    pub fn set_universe(&self, id: uuid::Uuid, universe: u16) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        let setup = store
            .setups
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("setup {id} not found"))?;
        setup.universe = normalize_universe(universe);
        setup.dirty = true;
        Ok(())
    }

    /// Delete a setup. Refuses to delete the only setup; if the active setup is
    /// removed, the first remaining one becomes active. A setup the cloud has
    /// seen leaves a pending tombstone so the delete propagates to other devices.
    pub fn delete(&self, id: uuid::Uuid) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        if store.setups.len() <= 1 {
            return Err("can't delete the only setup".into());
        }
        let Some(pos) = store.setups.iter().position(|s| s.id == id) else {
            return Err(format!("setup {id} not found"));
        };
        let removed = store.setups.remove(pos);
        if removed.updated_at.is_some() {
            store.pending_deletes.push(PendingDelete {
                setup_id: id,
                base_updated_at: removed.updated_at,
            });
        }
        if store.active_setup_id == id {
            store.active_setup_id = store.setups[0].id;
        }
        Ok(())
    }

    pub fn set_active(&self, id: uuid::Uuid) -> Result<(), String> {
        let mut store = self.store.lock().unwrap();
        if !store.setups.iter().any(|s| s.id == id) {
            return Err(format!("setup {id} not found"));
        }
        store.active_setup_id = id;
        Ok(())
    }

    // -- cloud sync helpers (driven by `crate::cloud`) --

    /// Setups with changes the cloud doesn't have yet (clones, for pushing).
    pub fn dirty_for_push(&self) -> Vec<Setup> {
        let store = self.store.lock().unwrap();
        store.setups.iter().filter(|s| s.needs_push()).cloned().collect()
    }

    /// Pending delete tombstones to push.
    pub fn pending_deletes(&self) -> Vec<PendingDelete> {
        self.store.lock().unwrap().pending_deletes.clone()
    }

    /// Record a successful push: store the server timestamp and clear dirty.
    pub fn mark_pushed(&self, id: uuid::Uuid, updated_at: i64) {
        let mut store = self.store.lock().unwrap();
        if let Some(s) = store.setups.iter_mut().find(|s| s.id == id) {
            s.updated_at = Some(updated_at);
            s.dirty = false;
        }
    }

    /// Drop a delivered delete tombstone.
    pub fn clear_pending_delete(&self, setup_id: uuid::Uuid) {
        self.store
            .lock()
            .unwrap()
            .pending_deletes
            .retain(|d| d.setup_id != setup_id);
    }

    /// Snapshot of all setups (clones), to feed reconcile.
    pub fn all(&self) -> Vec<Setup> {
        self.store.lock().unwrap().setups.clone()
    }

    /// The account email this store is currently synced with, if any.
    pub fn bound_email(&self) -> Option<String> {
        self.store.lock().unwrap().bound_email.clone()
    }

    /// Replace the working set with reconciled setups and bind it to `email`.
    /// Keeps the store non-empty and `active_setup_id` valid.
    pub fn replace_with_merged(&self, merged: Vec<Setup>, email: String) {
        let mut store = self.store.lock().unwrap();
        store.bound_email = Some(email);
        if merged.is_empty() {
            let seed = new_setup("Home", DEFAULT_UNIVERSE, fixture::default_fixtures());
            store.active_setup_id = seed.id;
            store.setups = vec![seed];
            return;
        }
        let active_ok = merged.iter().any(|s| s.id == store.active_setup_id);
        store.setups = merged;
        if !active_ok {
            store.active_setup_id = store.setups[0].id;
        }
    }

    /// Forget cloud binding and sync metadata so a *different* account signing in
    /// on this device can't push the previous user's setups into their cloud.
    pub fn reset_for_new_account(&self) {
        let mut store = self.store.lock().unwrap();
        store.bound_email = None;
        store.pending_deletes.clear();
        for s in &mut store.setups {
            s.updated_at = None;
            s.dirty = false;
        }
    }
}

fn clean_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("setup name can't be empty".into());
    }
    Ok(name)
}

// --- persistence (app_config_dir/setups.json) -------------------------------

fn config_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    app.path().app_config_dir().ok()
}

/// Load the store from disk, migrating a legacy `patch.json` if that's all that
/// exists, and falling back to the default store on first run or unreadable data.
pub fn load<R: Runtime>(app: &tauri::AppHandle<R>) -> LuxSetups {
    match config_dir(app) {
        Some(dir) => load_from_dir(&dir).into(),
        None => SetupStore::default().into(),
    }
}

/// The filesystem orchestration behind [`load`], split out so it can be exercised
/// against a real temp directory in tests without a Tauri app handle.
fn load_from_dir(dir: &Path) -> SetupStore {
    let path = dir.join("setups.json");

    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(json) => match parse_store(&json) {
                Ok(mut store) => {
                    migrate_version(&mut store);
                    reconcile(&mut store);
                    return store;
                }
                // A present-but-corrupt store is backed up, never silently
                // overwritten, and we do *not* fall back to the legacy patch.
                Err(e) => {
                    log::warn!("setups.json unreadable ({e}); backing it up and starting fresh");
                    backup_corrupt(&path);
                    return SetupStore::default();
                }
            },
            Err(e) => {
                log::warn!("could not read setups.json ({e}); using default store");
                return SetupStore::default();
            }
        }
    }

    // No store yet — migrate a legacy patch.json if one is present.
    let legacy = dir.join("patch.json");
    if legacy.exists() {
        let json = std::fs::read_to_string(&legacy).unwrap_or_default();
        let store = migrate_from_legacy(&json);
        write_store(&path, &store);
        // Keep the old file as a backup rather than deleting user data.
        let _ = std::fs::rename(&legacy, dir.join("patch.json.migrated"));
        log::info!("migrated patch.json -> setups.json (setup \"Home\")");
        return store;
    }

    // First run.
    SetupStore::default()
}

/// Persist the current store. Best-effort (logs on failure).
pub fn save<R: Runtime>(app: &tauri::AppHandle<R>, setups: &LuxSetups) {
    let Some(dir) = config_dir(app) else { return };
    write_store(&dir.join("setups.json"), &setups.snapshot());
}

fn write_store(path: &Path, store: &SetupStore) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                log::warn!("could not persist setups: {e}");
            }
        }
        Err(e) => log::warn!("could not serialize setups: {e}"),
    }
}

/// Move a corrupt store aside to `setups.json.corrupt-N` (first free N) so the
/// user's data is preserved for inspection instead of being clobbered.
fn backup_corrupt(path: &Path) {
    for n in 0..1000 {
        let candidate = PathBuf::from(format!("{}.corrupt-{n}", path.display()));
        if !candidate.exists() {
            if let Err(e) = std::fs::rename(path, &candidate) {
                log::warn!("could not back up corrupt setups.json: {e}");
            }
            return;
        }
    }
    log::warn!("too many corrupt setups.json backups; leaving the file in place");
}

// --- pure helpers (unit-tested without the filesystem) ----------------------

fn parse_store(json: &str) -> Result<SetupStore, serde_json::Error> {
    serde_json::from_str::<SetupStore>(json)
}

/// Wrap a legacy bare `Vec<Fixture>` patch into a single "Home" setup.
/// Unreadable legacy JSON degrades to an empty patch rather than losing the app.
fn migrate_from_legacy(json: &str) -> SetupStore {
    let fixtures = serde_json::from_str::<Vec<Fixture>>(json).unwrap_or_default();
    SetupStore::single(new_setup("Home", DEFAULT_UNIVERSE, fixtures))
}

/// Bring an older store up to the current schema version. No-op at v1; the hook
/// is here so the next bump has an obvious home.
fn migrate_version(store: &mut SetupStore) {
    // future: match store.version { 1 => { /* v1 -> v2 */ }, _ => {} }
    store.version = STORE_VERSION;
}

/// Repair invariants a hand-edited or partially-written store could violate: at
/// least one setup exists, and `active_setup_id` points at a real one.
fn reconcile(store: &mut SetupStore) {
    if store.setups.is_empty() {
        let setup = new_setup("Home", DEFAULT_UNIVERSE, fixture::default_fixtures());
        store.active_setup_id = setup.id;
        store.setups.push(setup);
        return;
    }
    if !store.setups.iter().any(|s| s.id == store.active_setup_id) {
        store.active_setup_id = store.setups[0].id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_store_is_one_home_setup_with_rgbaw() {
        let s = SetupStore::default();
        assert_eq!(s.version, STORE_VERSION);
        assert_eq!(s.setups.len(), 1);
        assert_eq!(s.setups[0].name, "Home");
        assert_eq!(s.setups[0].universe, 1);
        assert_eq!(s.active_setup_id, s.setups[0].id);
        assert_eq!(s.setups[0].fixtures.len(), 1);
        assert_eq!(s.setups[0].fixtures[0].channels.len(), 6);
    }

    #[test]
    fn migrates_legacy_patch_into_home_setup() {
        let legacy = r#"[
            {"id":"00000000-0000-0000-0000-000000000001","name":"Left","address":1,
             "channels":[{"role":"Red","label":"Red"}]},
            {"id":"00000000-0000-0000-0000-000000000002","name":"Right","address":2,
             "channels":[{"role":"Blue","label":"Blue"}]}
        ]"#;
        let store = migrate_from_legacy(legacy);
        assert_eq!(store.setups.len(), 1);
        assert_eq!(store.setups[0].name, "Home");
        assert_eq!(store.setups[0].fixtures.len(), 2);
        assert_eq!(store.active_setup_id, store.setups[0].id);
        assert_eq!(store.version, STORE_VERSION);
    }

    #[test]
    fn migrating_unreadable_legacy_yields_empty_home() {
        let store = migrate_from_legacy("not json at all");
        assert_eq!(store.setups.len(), 1);
        assert_eq!(store.setups[0].name, "Home");
        assert!(store.setups[0].fixtures.is_empty());
    }

    #[test]
    fn reconcile_fixes_dangling_active_id() {
        let mut store = SetupStore::default();
        store.active_setup_id = uuid::Uuid::new_v4(); // points at nothing
        reconcile(&mut store);
        assert_eq!(store.active_setup_id, store.setups[0].id);
    }

    #[test]
    fn reconcile_reseeds_empty_store() {
        let mut store = SetupStore::default();
        store.setups.clear();
        reconcile(&mut store);
        assert_eq!(store.setups.len(), 1);
        assert!(store.setups.iter().any(|s| s.id == store.active_setup_id));
    }

    #[test]
    fn store_round_trips_through_json() {
        let store = SetupStore::default();
        let json = serde_json::to_string(&store).unwrap();
        let back = parse_store(&json).unwrap();
        assert_eq!(back.setups.len(), store.setups.len());
        assert_eq!(back.active_setup_id, store.active_setup_id);
        assert_eq!(back.user_id, store.user_id);
        assert_eq!(back.version, store.version);
    }

    #[test]
    fn create_rename_delete_and_switch() {
        let setups: LuxSetups = SetupStore::default().into();
        let home = setups.active_id();

        let church = setups.create("Church".into(), 2).unwrap();
        assert_eq!(setups.summaries().len(), 2);

        setups.rename(church, "Sanctuary".into()).unwrap();
        assert!(setups.summaries().iter().any(|s| s.name == "Sanctuary"));

        setups.set_active(church).unwrap();
        assert_eq!(setups.active_id(), church);
        assert_eq!(setups.active_universe(), 2);

        // Deleting the active setup reassigns active to a remaining one.
        setups.delete(church).unwrap();
        assert_eq!(setups.active_id(), home);

        // Can't delete the last remaining setup.
        assert!(setups.delete(home).is_err());
    }

    #[test]
    fn fixture_ops_target_the_active_setup() {
        let setups: LuxSetups = SetupStore::default().into();
        // Default Home holds the RGBAW fixture on slots 1..=6; add another at 7.
        let dimmer = vec![ChannelDef {
            role: crate::colors::LuxLabelColor::Brightness,
            label: "Dimmer".into(),
        }];
        setups.add_fixture("Spot".into(), 7, dimmer).unwrap();
        assert_eq!(setups.active_fixtures().len(), 2);

        // A freshly-created setup starts empty and is unaffected.
        let work = setups.create("Work".into(), 1).unwrap();
        setups.set_active(work).unwrap();
        assert!(setups.active_fixtures().is_empty());
    }

    #[test]
    fn create_rejects_blank_name_and_clamps_universe() {
        let setups: LuxSetups = SetupStore::default().into();
        assert!(setups.create("   ".into(), 1).is_err());
        let id = setups.create("Edge".into(), 0).unwrap(); // 0 clamps up to 1
        setups.set_active(id).unwrap();
        assert_eq!(setups.active_universe(), 1);
    }

    // -- `load_from_dir` against a real (temp) directory --

    /// A fresh, uniquely-named temp directory; the caller removes it when done.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lux-test-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_from_dir_first_run_is_default_without_writing() {
        let dir = temp_dir("firstrun");
        let store = load_from_dir(&dir);
        assert_eq!(store.setups.len(), 1);
        assert_eq!(store.setups[0].name, "Home");
        // Matches the original behaviour: nothing is written until a mutation.
        assert!(!dir.join("setups.json").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_dir_migrates_a_real_legacy_patch() {
        let dir = temp_dir("migrate");
        std::fs::write(
            dir.join("patch.json"),
            r#"[{"id":"00000000-0000-0000-0000-000000000001","name":"Left","address":1,
                "channels":[{"role":"Red","label":"Red"}]}]"#,
        )
        .unwrap();

        let store = load_from_dir(&dir);
        assert_eq!(store.setups.len(), 1);
        assert_eq!(store.setups[0].name, "Home");
        assert_eq!(store.setups[0].fixtures.len(), 1);
        assert_eq!(store.setups[0].fixtures[0].name, "Left");

        // setups.json is written; the legacy file is preserved as a backup.
        assert!(dir.join("setups.json").exists());
        assert!(!dir.join("patch.json").exists());
        assert!(dir.join("patch.json.migrated").exists());

        // Idempotent: a second load reads the new store, not the (gone) legacy.
        let again = load_from_dir(&dir);
        assert_eq!(again.active_setup_id, store.active_setup_id);
        assert_eq!(again.setups[0].fixtures.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_dir_reads_back_an_existing_store() {
        let dir = temp_dir("existing");
        let original: LuxSetups = SetupStore::default().into();
        original.create("Church".into(), 5).unwrap();
        write_store(&dir.join("setups.json"), &original.snapshot());

        let store = load_from_dir(&dir);
        assert_eq!(store.setups.len(), 2);
        assert!(store.setups.iter().any(|s| s.name == "Church" && s.universe == 5));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_dir_backs_up_a_corrupt_store() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join("setups.json"), "{ not valid json").unwrap();

        let store = load_from_dir(&dir);
        assert_eq!(store.setups[0].name, "Home"); // fell back to default
        // The bad file is moved aside, never clobbered, and not re-read.
        assert!(dir.join("setups.json.corrupt-0").exists());
        assert!(!dir.join("setups.json").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
