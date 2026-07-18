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

pub fn default_config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("config.json"))
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
    }

    #[test]
    fn node_config_defaults_priority_below_surfaces() {
        let cfg: NodeConfig =
            serde_json::from_str(r#"{"setupId":"s-1","universe":1}"#).expect("parses");
        assert_eq!(cfg.priority, 90);
        assert!(cfg.interface.is_none());
    }
}
