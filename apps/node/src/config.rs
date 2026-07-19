//! Node configuration: the embedded production endpoints (same generated file
//! the apps embed — environment values are data, never code), the node's own
//! small config file, and the stored session.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The generated production endpoints, embedded at compile time from the same
/// file the desktop embeds (single source of truth; CI drift-gates it against
/// applied Terraform state). Never hand-edit the JSON.
const ENDPOINTS_JSON: &str = include_str!("../../desktop/src-tauri/endpoints.prod.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoints {
    pub cognito_region: String,
    pub cognito_user_pool_id: String,
    pub cognito_app_client_id: String,
    pub nudge_endpoint: String,
    pub sync_url: String,
    /// Base URL of the auth service (Sign in with Apple + `/auth/device/*`) —
    /// the node's pairing endpoint.
    pub apple_auth_url: String,
    /// The `lux-node-device` Cognito app client the pairing grant mints on;
    /// the node refreshes against it (recorded in [`StoredSession::client_id`]).
    pub cognito_device_client_id: String,
}

pub fn endpoints() -> Result<Endpoints, String> {
    serde_json::from_str(ENDPOINTS_JSON).map_err(|e| format!("embedded endpoints unreadable: {e}"))
}

/// `lux-node.json`: which setup this node applies, and how it transmits.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeConfig {
    /// The setup whose ctl frames this node applies (the UUID from the app's
    /// setups; the phone/desktop shows it in the sync record).
    pub setup_id: String,
    /// sACN universe to transmit on.
    pub universe: u16,
    /// Optional local NIC IPv4 to egress multicast from (multi-homed hosts).
    #[serde(default)]
    pub interface: Option<String>,
    /// E1.31 per-packet priority. Defaults below the surfaces' 100 so a human
    /// hand on a fader anywhere on the LAN overrides the node.
    #[serde(default = "default_priority")]
    pub priority: u8,
}

fn default_priority() -> u8 {
    90
}

pub fn load_node_config(path: &Path) -> Result<NodeConfig, String> {
    let json = fs::read_to_string(path)
        .map_err(|e| format!("could not read node config {}: {e}", path.display()))?;
    serde_json::from_str(&json).map_err(|e| format!("bad node config {}: {e}", path.display()))
}

/// The stored session: enough to restore on boot. Written 0600 — the refresh
/// token is the credential (a headless box has no OS keychain to lean on).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSession {
    pub email: String,
    pub refresh_token: String,
    /// Which app client minted this session. Absent (password-era sessions)
    /// means the interactive client from the embedded endpoints; device
    /// pairing writes the device client here, and refresh must match the
    /// minting client or Cognito rejects the token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

/// `$XDG_CONFIG_HOME/lux-node` (or `~/.config/lux-node`).
pub fn config_dir() -> Result<PathBuf, String> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("lux-node"));
        }
    }
    std::env::var("HOME")
        .map(|home| PathBuf::from(home).join(".config").join("lux-node"))
        .map_err(|_| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())
}

pub fn session_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("session.json"))
}

/// Is a session already stored? An absent session is the trigger for the
/// unpaired-boot state machine — `run` waits to be claimed instead of dying.
pub fn session_exists() -> Result<bool, String> {
    Ok(session_path()?.exists())
}

/// The paired setup binding lives here, not in the root-owned
/// `/etc/lux-node/config.json`: the service runs as `lux-node` under
/// `ProtectSystem=full`, so the approve step's choice lands in the state dir.
/// `run`'s precedence: explicit `--config` file → this → unpaired-wait.
pub fn node_binding_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("node.json"))
}

/// Which setup-binding source `run` uses. Pure, so the precedence is unit-tested
/// (`--config` file → paired state-dir `node.json` → none).
#[derive(Debug, PartialEq, Eq)]
pub enum Binding {
    /// An explicit `--config <file>` that exists on disk (today's installs).
    Explicit,
    /// The paired state-dir `node.json` (the approve step's choice).
    StateDir,
    /// No binding at all — the box must pair (or be given `--config`) first.
    None,
}

pub fn binding_choice(explicit_config_exists: bool, node_json_exists: bool) -> Binding {
    if explicit_config_exists {
        Binding::Explicit
    } else if node_json_exists {
        Binding::StateDir
    } else {
        Binding::None
    }
}

/// Persist the setup binding the approver chose (`{setupId, universe}`), the
/// same minimal shape `install` writes to `/etc/lux-node/config.json`;
/// `interface`/`priority` stay at their [`NodeConfig`] defaults.
pub fn save_node_binding(setup_id: &str, universe: u16) -> Result<(), String> {
    let path = node_binding_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let json = serde_json::json!({ "setupId": setup_id, "universe": universe });
    fs::write(&path, format!("{json:#}\n")).map_err(|e| format!("write {}: {e}", path.display()))
}

/// The node's stable self-generated id (a uuid), persisted in the state dir so
/// a re-registering node supersedes its own earlier pairing codes rather than
/// piling up new pending entries. Created on first read.
pub fn load_or_create_device_id() -> Result<String, String> {
    let path = config_dir()?.join("device_id");
    if let Ok(existing) = fs::read_to_string(&path) {
        let id = existing.trim();
        if !id.is_empty() {
            return Ok(id.to_owned());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    fs::write(&path, format!("{id}\n")).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(id)
}

pub fn load_session() -> Result<StoredSession, String> {
    let path = session_path()?;
    let json = fs::read_to_string(&path).map_err(|e| {
        format!(
            "no stored session at {} ({e}); run `lux-node login <email>` first",
            path.display()
        )
    })?;
    serde_json::from_str(&json).map_err(|e| format!("stored session unreadable: {e}"))
}

pub fn save_session(session: &StoredSession) -> Result<(), String> {
    let path = session_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    write_private(&path, json.as_bytes())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_endpoints_parse() {
        let e = endpoints().expect("endpoints parse");
        assert!(!e.cognito_region.is_empty());
        assert!(!e.nudge_endpoint.is_empty());
        assert!(e.sync_url.starts_with("https://"));
        assert!(e.apple_auth_url.starts_with("https://"));
        assert!(!e.cognito_device_client_id.is_empty());
    }

    #[test]
    fn node_config_defaults_priority_below_surfaces() {
        let cfg: NodeConfig =
            serde_json::from_str(r#"{"setupId":"s-1","universe":1}"#).expect("parses");
        assert_eq!(cfg.priority, 90);
        assert!(cfg.interface.is_none());
    }

    #[test]
    fn binding_precedence_explicit_over_state_over_none() {
        // An existing --config wins even when a paired binding is present.
        assert_eq!(binding_choice(true, true), Binding::Explicit);
        assert_eq!(binding_choice(true, false), Binding::Explicit);
        // No usable --config: fall back to the paired state-dir binding.
        assert_eq!(binding_choice(false, true), Binding::StateDir);
        // Neither: the box hasn't been paired and wasn't given a config.
        assert_eq!(binding_choice(false, false), Binding::None);
    }

    #[test]
    fn node_binding_round_trips_through_load_node_config() {
        // What `save_node_binding` writes must parse back with the run loop's
        // loader and pick up the priority/interface defaults.
        let dir = std::env::temp_dir().join(format!("lux-node-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("node.json");
        let json = serde_json::json!({ "setupId": "s-42", "universe": 7u16 });
        std::fs::write(&path, format!("{json:#}\n")).expect("write");
        let cfg = load_node_config(&path).expect("parses");
        assert_eq!(cfg.setup_id, "s-42");
        assert_eq!(cfg.universe, 7);
        assert_eq!(cfg.priority, 90);
        assert!(cfg.interface.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
