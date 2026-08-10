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
    /// Whether the web (browser) Sign in with Apple flow is provisioned — the
    /// `.dmg`/dev fallback. True once the Services ID + its verified domain are
    /// live; the desktop lights its web Apple button on it. Default-false so a
    /// file that predates the field keeps the feature dark.
    pub apple_web_enabled: bool,
    /// Base URL of the lux-plan-bridge Function URL (the Planning Center
    /// bridge). Absent until the first release after that service's Terraform
    /// applied — the endpoints file only carries outputs that exist in applied
    /// state — and empty means the `/plan` route stays dark, which is the
    /// correct state for every build that predates the bridge.
    pub plan_bridge_url: String,
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
///
/// The local file is destructured rather than read field by field, so a field
/// added to [`Endpoints`] cannot quietly skip this function — the compiler
/// names the one with no decision yet. An omission here is invisible until a
/// dev stack's override is ignored and the app talks to production instead.
fn overlay(base: &mut Endpoints, local: Endpoints) {
    fn take(base: &mut String, local: String) {
        if !local.is_empty() {
            *base = local;
        }
    }
    let Endpoints {
        cognito_region,
        cognito_user_pool_id,
        cognito_app_client_id,
        cognito_device_client_id,
        sync_url,
        nudge_endpoint,
        apple_auth_url,
        apple_web_enabled,
        plan_bridge_url,
        remote_control,
        sacn_interface,
    } = local;
    take(&mut base.cognito_region, cognito_region);
    take(&mut base.cognito_user_pool_id, cognito_user_pool_id);
    take(&mut base.cognito_app_client_id, cognito_app_client_id);
    take(&mut base.cognito_device_client_id, cognito_device_client_id);
    take(&mut base.sync_url, sync_url);
    take(&mut base.nudge_endpoint, nudge_endpoint);
    take(&mut base.apple_auth_url, apple_auth_url);
    take(&mut base.plan_bridge_url, plan_bridge_url);
    take(&mut base.sacn_interface, sacn_interface);
    // A bool has no "absent" to tell apart from `false`, so this override only
    // runs one way: a local file can light the web Apple flow up on a dev
    // machine, and cannot switch prod's off.
    if apple_web_enabled {
        base.apple_web_enabled = true;
    }
    if remote_control.is_some() {
        base.remote_control = remote_control;
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

    /// Every field the local file can carry actually overrides. One missing
    /// `take` pins a dev run to production for that one subsystem and says
    /// nothing about it, so the whole set is asserted rather than a sample.
    #[test]
    fn every_field_a_dev_stack_sets_overrides_the_embedded_one() {
        let mut base = Endpoints {
            cognito_region: "prod".into(),
            cognito_user_pool_id: "prod".into(),
            cognito_app_client_id: "prod".into(),
            cognito_device_client_id: "prod".into(),
            sync_url: "prod".into(),
            nudge_endpoint: "prod".into(),
            apple_auth_url: "prod".into(),
            apple_web_enabled: false,
            plan_bridge_url: "prod".into(),
            remote_control: None,
            sacn_interface: "prod".into(),
        };
        overlay(
            &mut base,
            Endpoints {
                cognito_region: "local".into(),
                cognito_user_pool_id: "local".into(),
                cognito_app_client_id: "local".into(),
                cognito_device_client_id: "local".into(),
                sync_url: "local".into(),
                nudge_endpoint: "local".into(),
                apple_auth_url: "local".into(),
                apple_web_enabled: true,
                plan_bridge_url: "local".into(),
                remote_control: Some(RemoteControl {
                    endpoint: "local".into(),
                    device_id: "lux-dev".into(),
                    cert_path: "cert.pem".into(),
                    key_path: "key.pem".into(),
                    root_ca_path: "ca.pem".into(),
                }),
                sacn_interface: "local".into(),
            },
        );

        for (field, value) in [
            ("cognitoRegion", &base.cognito_region),
            ("cognitoUserPoolId", &base.cognito_user_pool_id),
            ("cognitoAppClientId", &base.cognito_app_client_id),
            ("cognitoDeviceClientId", &base.cognito_device_client_id),
            ("syncUrl", &base.sync_url),
            ("nudgeEndpoint", &base.nudge_endpoint),
            ("appleAuthUrl", &base.apple_auth_url),
            ("planBridgeUrl", &base.plan_bridge_url),
            ("sacnInterface", &base.sacn_interface),
        ] {
            assert_eq!(value, "local", "{field} kept the embedded value");
        }
        assert!(base.apple_web_enabled);
        assert!(base.remote_control.is_some());
    }
}
