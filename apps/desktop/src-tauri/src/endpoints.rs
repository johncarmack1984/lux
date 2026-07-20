//! Where this build points: environment configuration as data, never code.
//!
//! `endpoints.prod.json` is machine-generated from Terraform outputs
//! (`scripts/gen-endpoints`), committed, drift-gated in CI (infra PR plans and
//! the release apply both regenerate and diff it), and embedded here at
//! compile time — so the code stays environment-agnostic, release builds carry
//! their production config as data, and a stale value fails a check instead of
//! shipping. Never hand-edit it.
//!
//! An optional, gitignored `endpoints.local.json` beside it (read from the
//! working directory, so it applies to `tauri dev` runs) overrides any subset
//! of fields — a dev stack, a test pool — and is also where the dev-machine
//! remote-control listener is configured. There are no env files and no env
//! vars; this module is the only place environment values enter the app.
//!
//! Empty or missing fields mean "not configured": the owning subsystem no-ops
//! and logs, never panics — identity, sync, and nudges must degrade to
//! local-only operation because they never sit in the live DMX path.

use std::sync::OnceLock;

use serde::Deserialize;

/// The environment this build talks to. All fields optional-by-emptiness.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Endpoints {
    pub cognito_region: String,
    pub cognito_user_pool_id: String,
    pub cognito_app_client_id: String,
    /// The `lux-node-device` app client the headless pairing grant mints on.
    /// Present from the first release after that client's Terraform applied;
    /// only lux-node refreshes against it, but the app carries it so the
    /// generated endpoints file stays one shape across both embedders.
    pub cognito_device_client_id: String,
    pub sync_url: String,
    pub nudge_endpoint: String,
    /// Base URL of the lux-apple-auth Function URL (Sign in with Apple).
    /// Absent until the first release after the service's Terraform applied —
    /// the endpoints file only carries outputs that exist in applied state —
    /// and empty means the feature stays dark.
    pub apple_auth_url: String,
    /// Dev-machine remote control (device identity + mTLS material); only ever
    /// present in `endpoints.local.json` — the generated prod file never
    /// carries it, so plain installs have no remote-control surface.
    pub remote_control: Option<RemoteControl>,
    /// Advanced, machine-specific: which local NIC sends sACN multicast on a
    /// multi-homed machine (an IPv4 address). Local-only, normally absent —
    /// the OS routes out the interface the node is on.
    pub sacn_interface: String,
}

/// Config for the AWS IoT remote-control listener (`remote.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteControl {
    pub endpoint: String,
    #[serde(default = "default_device_id")]
    pub device_id: String,
    pub cert_path: String,
    pub key_path: String,
    pub root_ca_path: String,
}

fn default_device_id() -> String {
    "lux-1".into()
}

/// The effective configuration: the embedded prod file with any local
/// overrides applied. Computed once.
pub fn effective() -> &'static Endpoints {
    static CELL: OnceLock<Endpoints> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut endpoints = prod();
        if let Some(local) = local() {
            overlay(&mut endpoints, local);
        }
        endpoints
    })
}

/// The embedded production config. A parse failure here is a build-system bug
/// (the committed file is machine-generated and CI-tested), but degrade to
/// "nothing configured" rather than panicking in a lighting app.
fn prod() -> Endpoints {
    serde_json::from_str(include_str!("../endpoints.prod.json")).unwrap_or_else(|e| {
        log::error!("embedded endpoints.prod.json is invalid ({e}); cloud features disabled");
        Endpoints::default()
    })
}

/// `endpoints.local.json` from the working directory, if present (dev runs
/// start in `src-tauri/`, where the gitignored file lives).
fn local() -> Option<Endpoints> {
    let raw = std::fs::read_to_string("endpoints.local.json").ok()?;
    match serde_json::from_str(&raw) {
        Ok(endpoints) => {
            log::info!("endpoints.local.json found; applying local overrides");
            Some(endpoints)
        }
        Err(e) => {
            log::warn!("ignoring malformed endpoints.local.json: {e}");
            None
        }
    }
}

/// Field-wise override: a non-empty local value wins, an empty/missing one
/// keeps prod. `remote_control` is local-only, so it carries over whole.
fn overlay(base: &mut Endpoints, local: Endpoints) {
    fn take(base: &mut String, local: String) {
        if !local.is_empty() {
            *base = local;
        }
    }
    take(&mut base.cognito_region, local.cognito_region);
    take(&mut base.cognito_user_pool_id, local.cognito_user_pool_id);
    take(&mut base.cognito_app_client_id, local.cognito_app_client_id);
    take(
        &mut base.cognito_device_client_id,
        local.cognito_device_client_id,
    );
    take(&mut base.sync_url, local.sync_url);
    take(&mut base.nudge_endpoint, local.nudge_endpoint);
    take(&mut base.apple_auth_url, local.apple_auth_url);
    take(&mut base.sacn_interface, local.sacn_interface);
    if local.remote_control.is_some() {
        base.remote_control = local.remote_control;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed prod file must parse and be fully populated — this is the
    /// keyless half of the drift gate (CI's credentialed half regenerates the
    /// file from Terraform state and diffs it).
    #[test]
    fn embedded_prod_endpoints_parse_and_are_populated() {
        let endpoints: Endpoints =
            serde_json::from_str(include_str!("../endpoints.prod.json")).expect("must parse");
        assert!(!endpoints.cognito_region.is_empty());
        assert!(!endpoints.cognito_user_pool_id.is_empty());
        assert!(!endpoints.cognito_app_client_id.is_empty());
        assert!(!endpoints.cognito_device_client_id.is_empty());
        assert!(!endpoints.sync_url.is_empty());
        assert!(!endpoints.nudge_endpoint.is_empty());
        assert!(!endpoints.apple_auth_url.is_empty());
        assert!(
            endpoints.remote_control.is_none(),
            "prod never configures remote control"
        );
    }

    #[test]
    fn overlay_prefers_non_empty_local_fields() {
        let mut base = Endpoints {
            cognito_region: "us-west-1".into(),
            sync_url: "https://prod.example/".into(),
            ..Endpoints::default()
        };
        overlay(
            &mut base,
            Endpoints {
                sync_url: "https://dev.example/".into(),
                ..Endpoints::default()
            },
        );
        assert_eq!(base.sync_url, "https://dev.example/");
        assert_eq!(base.cognito_region, "us-west-1"); // empty local field keeps prod
    }
}
