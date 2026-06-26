//! Cloud sync engine: push local setups to the lux-sync-api Function URL and
//! pull/merge remote ones, so a signed-in user's setups follow them across
//! devices.
//!
//! Local-authoritative: this only moves *config* (the setups), never live DMX
//! levels — output stays driven by [`crate::buffer`]. Disabled unless accounts
//! are configured, signed in, and `LUX_SYNC_URL` is set (mirrors the rest of the
//! app's "configured via env, else no-op" contract).
//!
//! - **Push** (on every local mutation): each dirty setup is conditional-written
//!   with its last-known server `updatedAt` as the optimistic-concurrency base.
//! - **Pull** (on sign-in / startup restore): list the account's setups and
//!   [`reconcile`] them into the local store, per-setup last-writer-wins by
//!   server `updatedAt`, then flush anything still dirty. First sign-in claims
//!   the local setups into the account.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::account::LuxAccount;
use crate::cmd::CmdEvent;
use crate::fixture::Fixture;
use crate::setup::{LuxSetups, PendingDelete, Setup};

// --- sync status (drives the UI indicator) -----------------------------------

/// Coarse cloud-sync state for the nav indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    /// Signed out, or signed in with nothing left to sync.
    #[default]
    Idle,
    /// A push or pull is in flight.
    Syncing,
    /// The last cycle completed and everything is flushed.
    Synced,
    /// The last attempt couldn't reach the cloud; a backoff retry is running.
    Offline,
}

/// Tauri-managed sync state plus the cloud layer's concurrency guards.
#[derive(Default)]
pub struct LuxSync {
    state: Mutex<SyncState>,
    /// Held while a pull/push cycle runs, so focus-triggered syncs coalesce
    /// instead of stacking up.
    in_flight: AtomicBool,
    /// Held while a backoff retry loop is alive, so only one runs at a time.
    retrying: AtomicBool,
}

impl LuxSync {
    /// The current state, for the `sync_status` command and event payloads.
    pub fn snapshot(&self) -> SyncState {
        *self.state.lock().unwrap()
    }

    /// Move to `state` and emit the change to the UI.
    fn set(&self, app: &AppHandle, state: SyncState) {
        *self.state.lock().unwrap() = state;
        let _ = CmdEvent::SyncStatusChanged { state }.emit(app);
    }
}

#[derive(Debug)]
enum SyncError {
    /// Token expired/invalid — refresh and retry.
    Unauthorized,
    /// Conditional write lost a race; reconcile on the next pull.
    Conflict,
    Other(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Unauthorized => write!(f, "unauthorized"),
            SyncError::Conflict => write!(f, "conflict"),
            SyncError::Other(e) => write!(f, "{e}"),
        }
    }
}

// --- wire types (match sync-api/src/store.rs) --------------------------------

#[derive(Deserialize)]
struct ListResponse {
    setups: Vec<CloudSetup>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudSetup {
    id: String,
    name: String,
    universe: u16,
    /// Opaque on the wire; deserialized into `Vec<Fixture>` by `cloud_to_setup`.
    fixtures: serde_json::Value,
    updated_at: i64,
    #[serde(default)]
    deleted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpsertBody<'a> {
    name: &'a str,
    universe: u16,
    fixtures: &'a Vec<Fixture>,
    base_updated_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteResponse {
    updated_at: i64,
}

// --- pure merge logic (unit-tested) -----------------------------------------

fn cloud_to_setup(c: &CloudSetup) -> Option<Setup> {
    Some(Setup {
        id: Uuid::parse_str(&c.id).ok()?,
        name: c.name.clone(),
        universe: c.universe,
        fixtures: serde_json::from_value(c.fixtures.clone()).ok()?,
        updated_at: Some(c.updated_at),
        dirty: false,
    })
}

/// Merge cloud setups into the local set, per-setup last-writer-wins by server
/// `updatedAt`. Local-only never-synced setups are kept (to be pushed); remote
/// tombstones remove; remote-only setups are added; a dirty local edit on the
/// latest base is kept and re-pushed, but loses to a newer remote change.
fn reconcile(local: Vec<Setup>, remote: &[CloudSetup]) -> Vec<Setup> {
    let remote_by_id: HashMap<Uuid, &CloudSetup> = remote
        .iter()
        .filter_map(|c| Uuid::parse_str(&c.id).ok().map(|id| (id, c)))
        .collect();
    let local_ids: HashSet<Uuid> = local.iter().map(|s| s.id).collect();

    let mut merged = Vec::new();
    for l in local {
        let Some(c) = remote_by_id.get(&l.id) else {
            // Not on the server: keep it (a never-synced setup gets pushed).
            merged.push(l);
            continue;
        };
        let base = l.updated_at.unwrap_or(i64::MIN);
        if c.deleted {
            // Remote delete wins unless we hold a strictly newer change.
            if c.updated_at < base {
                merged.push(l);
            }
        } else if l.dirty && c.updated_at <= base {
            // Our unpushed edit is on the latest base — keep it, re-push later.
            merged.push(l);
        } else if c.updated_at > base {
            // Remote is newer (LWW) — take it, falling back to local if it won't parse.
            merged.push(cloud_to_setup(c).unwrap_or(l));
        } else {
            // Already in sync.
            merged.push(l);
        }
    }

    for c in remote {
        let fresh = Uuid::parse_str(&c.id).map(|id| !local_ids.contains(&id)).unwrap_or(false);
        if fresh && !c.deleted {
            if let Some(s) = cloud_to_setup(c) {
                merged.push(s);
            }
        }
    }
    merged
}

// --- HTTP (one call each; owned token so the future is self-contained) -------

async fn list(client: &Client, base: &str, token: String) -> Result<Vec<CloudSetup>, SyncError> {
    let resp = client
        .get(format!("{base}/setups"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| SyncError::Other(e.to_string()))?;
    let body: ListResponse = read_json(resp).await?;
    Ok(body.setups)
}

async fn upsert(
    client: &Client,
    base: &str,
    token: String,
    setup: &Setup,
) -> Result<WriteResponse, SyncError> {
    let body = UpsertBody {
        name: &setup.name,
        universe: setup.universe,
        fixtures: &setup.fixtures,
        base_updated_at: setup.updated_at,
    };
    let resp = client
        .put(format!("{base}/setups/{}", setup.id))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| SyncError::Other(e.to_string()))?;
    read_json(resp).await
}

async fn tombstone(
    client: &Client,
    base: &str,
    token: String,
    delete: &PendingDelete,
) -> Result<(), SyncError> {
    let mut url = format!("{base}/setups/{}", delete.setup_id);
    if let Some(base_updated_at) = delete.base_updated_at {
        url.push_str(&format!("?baseUpdatedAt={base_updated_at}"));
    }
    let resp = client
        .delete(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| SyncError::Other(e.to_string()))?;
    let _: serde_json::Value = read_json(resp).await?;
    Ok(())
}

async fn read_json<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, SyncError> {
    match resp.status() {
        StatusCode::UNAUTHORIZED => Err(SyncError::Unauthorized),
        StatusCode::CONFLICT => Err(SyncError::Conflict),
        status if status.is_success() => {
            resp.json::<T>().await.map_err(|e| SyncError::Other(e.to_string()))
        }
        status => {
            let body = resp.text().await.unwrap_or_default();
            Err(SyncError::Other(format!("{status}: {body}")))
        }
    }
}

// --- orchestration -----------------------------------------------------------

async fn refresh(app: &AppHandle) -> Result<String, SyncError> {
    app.state::<LuxAccount>()
        .refresh_id_token()
        .await
        .map_err(SyncError::Other)
}

/// Push every dirty setup, then flush pending delete tombstones. On a 401 the
/// token is refreshed once and the call retried. Conflicts are left for the next
/// pull to reconcile. Persists the store afterward so cleared dirty flags stick.
async fn push_all(app: &AppHandle, client: &Client, base: &str, token: &mut String) {
    for setup in app.state::<LuxSetups>().dirty_for_push() {
        let mut result = upsert(client, base, token.clone(), &setup).await;
        if matches!(result, Err(SyncError::Unauthorized)) {
            if let Ok(fresh) = refresh(app).await {
                *token = fresh;
                result = upsert(client, base, token.clone(), &setup).await;
            }
        }
        match result {
            Ok(w) => app.state::<LuxSetups>().mark_pushed(setup.id, w.updated_at),
            Err(SyncError::Conflict) => {
                log::info!("push conflict for setup {}; reconciling on next pull", setup.id)
            }
            Err(e) => log::warn!("push failed for setup {}: {e}", setup.id),
        }
    }

    for delete in app.state::<LuxSetups>().pending_deletes() {
        let mut result = tombstone(client, base, token.clone(), &delete).await;
        if matches!(result, Err(SyncError::Unauthorized)) {
            if let Ok(fresh) = refresh(app).await {
                *token = fresh;
                result = tombstone(client, base, token.clone(), &delete).await;
            }
        }
        match result {
            // A conflict means the setup changed remotely; drop the delete and let
            // the next pull surface the remote state.
            Ok(()) | Err(SyncError::Conflict) => {
                app.state::<LuxSetups>().clear_pending_delete(delete.setup_id)
            }
            Err(e) => log::warn!("delete push failed for {}: {e}", delete.setup_id),
        }
    }

    crate::setup::save(app, &app.state::<LuxSetups>());
}

/// Pull the account's setups, reconcile them into the local store (claiming
/// local setups on first sign-in), broadcast the new state, then push whatever
/// is still dirty. A different account signing in replaces the working set with
/// theirs rather than leaking the previous user's setups.
async fn pull_and_push(app: &AppHandle) {
    let account = app.state::<LuxAccount>();
    if !account.signed_in() {
        return;
    }
    let (Some(base), Some(email), Some(mut token)) =
        (account.sync_url(), account.email(), account.current_id_token())
    else {
        return;
    };
    drop(account);

    let different_account = app
        .state::<LuxSetups>()
        .bound_email()
        .is_some_and(|bound| bound != email);

    let client = Client::new();
    let remote = match list(&client, &base, token.clone()).await {
        Err(SyncError::Unauthorized) => {
            let Ok(fresh) = refresh(app).await else {
                log::warn!("sync pull failed: could not refresh token");
                return;
            };
            token = fresh;
            list(&client, &base, token.clone()).await
        }
        other => other,
    }
    .unwrap_or_else(|e| {
        log::warn!("sync pull failed: {e}");
        Vec::new()
    });

    // For a different account, start from their cloud state only (don't merge the
    // previous user's local setups, and don't push them to this account).
    let local = if different_account {
        app.state::<LuxSetups>().reset_for_new_account();
        Vec::new()
    } else {
        app.state::<LuxSetups>().all()
    };

    let merged = reconcile(local, &remote);
    app.state::<LuxSetups>().replace_with_merged(merged, email);
    crate::cmd::broadcast_synced_state(app);

    push_all(app, &client, &base, &mut token).await;
}

fn syncable(app: &AppHandle) -> bool {
    let account = app.state::<LuxAccount>();
    account.signed_in() && account.sync_url().is_some()
}

/// True when nothing is waiting to reach the cloud (no dirty setups, no pending
/// delete tombstones).
fn fully_flushed(app: &AppHandle) -> bool {
    let setups = app.state::<LuxSetups>();
    setups.dirty_for_push().is_empty() && setups.pending_deletes().is_empty()
}

/// Close out a push/pull cycle: `Synced` if everything reached the cloud, else
/// `Offline` plus a background backoff retry so it keeps trying on its own.
fn finish_cycle(app: &AppHandle) {
    if fully_flushed(app) {
        app.state::<LuxSync>().set(app, SyncState::Synced);
    } else {
        app.state::<LuxSync>().set(app, SyncState::Offline);
        schedule_retry(app);
    }
}

/// Flush local changes to the cloud in the background (called after a local
/// mutation). No-op unless signed in, sync configured, and something to push.
pub fn schedule_push(app: &AppHandle) {
    if !syncable(app) || fully_flushed(app) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (Some(base), Some(mut token)) = (
            app.state::<LuxAccount>().sync_url(),
            app.state::<LuxAccount>().current_id_token(),
        ) else {
            return;
        };
        app.state::<LuxSync>().set(&app, SyncState::Syncing);
        let client = Client::new();
        push_all(&app, &client, &base, &mut token).await;
        finish_cycle(&app);
    });
}

/// Pull + reconcile + push in the background (called on sign-in, startup restore,
/// and window focus). No-op unless signed in with sync configured. Coalesces: if
/// a cycle is already running this is a no-op — that cycle ends with a push that
/// picks up anything newly dirty.
pub fn schedule_sync(app: &AppHandle) {
    if !syncable(app) {
        return;
    }
    if app.state::<LuxSync>().in_flight.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        app.state::<LuxSync>().set(&app, SyncState::Syncing);
        pull_and_push(&app).await;
        finish_cycle(&app);
        app.state::<LuxSync>().in_flight.store(false, Ordering::SeqCst);
    });
}

/// Keep retrying the push with exponential backoff until everything is flushed,
/// the user signs out, or sync is disabled. Only one retry loop runs at a time,
/// and a fresh local mutation's push is picked up by the same loop (it re-reads
/// the dirty set each pass).
fn schedule_retry(app: &AppHandle) {
    if app.state::<LuxSync>().retrying.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        const BACKOFF_SECS: [u64; 5] = [5, 15, 30, 60, 120];
        let mut attempt = 0usize;
        loop {
            if !syncable(&app) || fully_flushed(&app) {
                break;
            }
            let wait = BACKOFF_SECS[attempt.min(BACKOFF_SECS.len() - 1)];
            tokio::time::sleep(Duration::from_secs(wait)).await;
            if !syncable(&app) {
                break;
            }
            let client = Client::new();
            let (Some(base), Some(mut token)) = (
                app.state::<LuxAccount>().sync_url(),
                app.state::<LuxAccount>().current_id_token(),
            ) else {
                break;
            };
            push_all(&app, &client, &base, &mut token).await;
            if fully_flushed(&app) {
                app.state::<LuxSync>().set(&app, SyncState::Synced);
                break;
            }
            attempt += 1;
        }
        app.state::<LuxSync>().retrying.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_setup(id: u128, name: &str, updated_at: Option<i64>, dirty: bool) -> Setup {
        Setup {
            id: Uuid::from_u128(id),
            name: name.into(),
            universe: 1,
            fixtures: vec![],
            updated_at,
            dirty,
        }
    }

    fn cloud_setup(id: u128, name: &str, updated_at: i64, deleted: bool) -> CloudSetup {
        CloudSetup {
            id: Uuid::from_u128(id).to_string(),
            name: name.into(),
            universe: 1,
            fixtures: serde_json::json!([]),
            updated_at,
            deleted,
        }
    }

    #[test]
    fn remote_only_is_added() {
        let merged = reconcile(vec![], &[cloud_setup(1, "Home", 100, false)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Home");
        assert_eq!(merged[0].updated_at, Some(100));
        assert!(!merged[0].dirty);
    }

    #[test]
    fn local_never_synced_is_kept_for_push() {
        let merged = reconcile(vec![local_setup(1, "Home", None, false)], &[]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].updated_at.is_none()); // still needs pushing
    }

    #[test]
    fn remote_newer_wins_when_not_dirty() {
        let local = vec![local_setup(1, "Old", Some(100), false)];
        let remote = [cloud_setup(1, "New", 200, false)];
        let merged = reconcile(local, &remote);
        assert_eq!(merged[0].name, "New");
        assert_eq!(merged[0].updated_at, Some(200));
    }

    #[test]
    fn dirty_local_on_latest_base_is_kept() {
        // Remote unchanged since our base (100) — our dirty edit survives to push.
        let local = vec![local_setup(1, "MyEdit", Some(100), true)];
        let remote = [cloud_setup(1, "Server", 100, false)];
        let merged = reconcile(local, &remote);
        assert_eq!(merged[0].name, "MyEdit");
        assert!(merged[0].dirty);
    }

    #[test]
    fn dirty_local_loses_to_newer_remote() {
        // Remote moved past our base — last-writer-wins gives it to the server.
        let local = vec![local_setup(1, "MyEdit", Some(100), true)];
        let remote = [cloud_setup(1, "Server", 200, false)];
        let merged = reconcile(local, &remote);
        assert_eq!(merged[0].name, "Server");
        assert!(!merged[0].dirty);
    }

    #[test]
    fn remote_tombstone_removes_local() {
        let local = vec![local_setup(1, "Home", Some(100), false)];
        let remote = [cloud_setup(1, "Home", 200, true)];
        let merged = reconcile(local, &remote);
        assert!(merged.is_empty());
    }

    #[test]
    fn claim_unions_local_and_remote() {
        // First sign-in: a local-only setup and a remote-only setup both survive.
        let local = vec![local_setup(1, "Local", None, false)];
        let remote = [cloud_setup(2, "Remote", 100, false)];
        let merged = reconcile(local, &remote);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|s| s.name == "Local"));
        assert!(merged.iter().any(|s| s.name == "Remote"));
    }

    /// End-to-end create → list → tombstone against the live sync API. Ignored by
    /// default (needs network + a real id token). Run with:
    /// ```sh
    /// LUX_SYNC_URL=… LUX_TEST_ID_TOKEN=… \
    /// cargo test cloud_round_trip_live -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "hits the live sync API; needs LUX_SYNC_URL + LUX_TEST_ID_TOKEN"]
    fn cloud_round_trip_live() {
        // reqwest 0.13 (rustls-no-provider) needs a process crypto provider before
        // building a Client; the app installs ring in lib.rs::run(), so mirror that here.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let base = std::env::var("LUX_SYNC_URL")
            .unwrap()
            .trim_end_matches('/')
            .to_string();
        let token = std::env::var("LUX_TEST_ID_TOKEN").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = Client::new();
            let id = Uuid::new_v4();
            let setup = Setup {
                id,
                name: "Sync Round-Trip".into(),
                universe: 7,
                fixtures: vec![],
                updated_at: None,
                dirty: true,
            };

            let written = upsert(&client, &base, token.clone(), &setup)
                .await
                .expect("upsert");
            assert!(written.updated_at > 0);

            let remote = list(&client, &base, token.clone()).await.expect("list");
            let found = remote
                .iter()
                .find(|c| c.id == id.to_string())
                .expect("setup present after upsert");
            assert_eq!(found.name, "Sync Round-Trip");
            assert_eq!(found.universe, 7);

            let delete = PendingDelete {
                setup_id: id,
                base_updated_at: Some(written.updated_at),
            };
            tombstone(&client, &base, token, &delete)
                .await
                .expect("tombstone");
        });
    }
}
