//! The guest half of shared control: what this device can *see* of a setup
//! someone else shared with it (docs/shared-control.md).
//!
//! A guest holds no copy of the owner's setup and never syncs one. Everything
//! it knows arrives on two retained topics in the owner's ctl space:
//!
//! - `…/setup/<id>/config` — the compiled setup, which is the guest surface's
//!   entire source of truth for what the desk looks like.
//! - `…/setup/<id>/state` — the owner applier's last-applied buffer, so a
//!   guest's faders open at the truth rather than at zero.
//!
//! That is deliberately all of it. Nothing here writes to disk, nothing here
//! joins the sync engine, and nothing survives sign-out — when a grant ends,
//! the guest's view of it ends with the process at the latest, and usually
//! sooner (the owner clears the retained config, and the authorizer stops
//! granting receive at the next reconnect).
//!
//! This module is read-only by construction: it subscribes, parses, and
//! stores. Publishing into an owner's space — presence and control frames —
//! is a separate concern and lives with the surface that does it.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime};

use crate::lock::LockPolicy;

/// Identifies one shared setup: whose space it lives in, and which setup.
type Key = (String, String);

/// One setup another account has shared with this one, as a surface sees it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SharedSetup {
    /// The owner's Cognito sub — the ctl topic space this grant addresses.
    pub owner_sub: String,
    /// How the owner appears in the list (their account email).
    pub owner_label: String,
    pub setup_id: String,
    /// The name recorded when the grant was created. The live name rides the
    /// compiled config; this is what the list shows before one arrives.
    pub setup_name: Option<String>,
    /// True once the owner's compiled config has arrived, which is what a
    /// surface needs before it can draw anything. False means the owner's
    /// applier has not published one — it may simply not be running.
    pub renderable: bool,
}

/// What this device can currently see of other people's setups.
///
/// Held in memory only. A restart re-learns all of it from `GET /shares` plus
/// the retained topics, which is the point: there is no local copy of someone
/// else's setup to go stale, leak, or need cleaning up when a grant ends.
#[derive(Default)]
pub struct LuxGuest {
    /// Grants received, from `GET /shares` — the authority on what may be
    /// opened at all. The retained topics are the *content*; this is the
    /// permission, and a config without a matching grant is ignored.
    grants: Mutex<Vec<lux_wire::shares::ReceivedGrant>>,
    /// Compiled setups, as published by each owner's applier.
    configs: Mutex<HashMap<Key, lux_wire::ctl::Config>>,
    /// Each shared setup's last-known buffer, from the owner's state echo.
    states: Mutex<HashMap<Key, Vec<u8>>>,
}

impl LuxGuest {
    /// The shared setups this device may open, newest grant first.
    pub fn shared_setups(&self) -> Vec<SharedSetup> {
        let configs = self.configs.lock_or_recover();
        let mut shared: Vec<SharedSetup> = self
            .grants
            .lock_or_recover()
            .iter()
            .map(|g| SharedSetup {
                owner_sub: g.owner_sub.clone(),
                owner_label: g.owner_label.clone(),
                setup_id: g.setup_id.clone(),
                setup_name: g.setup_name.clone(),
                renderable: configs.contains_key(&(g.owner_sub.clone(), g.setup_id.clone())),
            })
            .collect();
        shared.sort_by(|a, b| (&a.owner_label, &a.setup_id).cmp(&(&b.owner_label, &b.setup_id)));
        shared
    }

    /// The compiled setup for one shared setup, if its config has arrived.
    pub fn config(&self, owner_sub: &str, setup_id: &str) -> Option<lux_wire::ctl::Config> {
        self.configs
            .lock_or_recover()
            .get(&(owner_sub.to_owned(), setup_id.to_owned()))
            .cloned()
    }

    /// The owner applier's last-known buffer for one shared setup.
    pub fn state(&self, owner_sub: &str, setup_id: &str) -> Option<Vec<u8>> {
        self.states
            .lock_or_recover()
            .get(&(owner_sub.to_owned(), setup_id.to_owned()))
            .cloned()
    }

    /// Does a live grant cover this pair? The check every incoming payload
    /// passes before it is stored — the authorizer is the real gate, but a
    /// listener that trusted topics alone would keep rendering a setup whose
    /// grant ended until the connection happened to drop.
    fn granted(&self, owner_sub: &str, setup_id: &str) -> bool {
        self.grants
            .lock_or_recover()
            .iter()
            .any(|g| g.owner_sub == owner_sub && g.setup_id == setup_id)
    }
}

/// Adopt the grants from a `GET /shares` pull: remember them, and forget
/// everything belonging to a grant that is no longer in the list.
///
/// Forgetting is the half that matters. A revoked guest keeps its connection —
/// and therefore its old policy — until the authorizer's refresh window
/// expires, so the broker may still deliver on a topic it no longer has any
/// business reading. Dropping the content the moment the grant leaves the list
/// makes the surface honest well before the policy catches up.
pub fn adopt_grants<R: Runtime>(app: &AppHandle<R>, received: &[lux_wire::shares::ReceivedGrant]) {
    let guest = app.state::<LuxGuest>();
    let live: Vec<Key> = received
        .iter()
        .map(|g| (g.owner_sub.clone(), g.setup_id.clone()))
        .collect();
    *guest.grants.lock_or_recover() = received.to_vec();
    guest
        .configs
        .lock_or_recover()
        .retain(|key, _| live.contains(key));
    guest
        .states
        .lock_or_recover()
        .retain(|key, _| live.contains(key));
}

/// Store an owner's compiled setup. An empty payload is the owner clearing it
/// (the setup was deleted, or the last grant on it was revoked), which drops
/// the surface's ability to render rather than leaving a stale desk up.
pub fn receive_config<R: Runtime>(
    app: &AppHandle<R>,
    owner_sub: &str,
    setup_id: &str,
    payload: &[u8],
) {
    let guest = app.state::<LuxGuest>();
    if !guest.granted(owner_sub, setup_id) {
        log::debug!("ignoring a config for {owner_sub}/{setup_id}: no live grant");
        return;
    }
    let key = (owner_sub.to_owned(), setup_id.to_owned());
    if payload.is_empty() {
        guest.configs.lock_or_recover().remove(&key);
        return;
    }
    let config: lux_wire::ctl::Config = match serde_json::from_slice(payload) {
        Ok(config) => config,
        Err(e) => {
            log::warn!("ignoring an unreadable compiled setup from {owner_sub}: {e}");
            return;
        }
    };
    if config.v != lux_wire::ctl::VERSION {
        // The owner is on a newer app than this one — App Review lag makes
        // that the normal case, not an error. Drop it and render nothing
        // rather than guess at a shape we don't know.
        log::info!(
            "dropping a compiled setup from {owner_sub} with unknown version {}",
            config.v
        );
        return;
    }
    guest.configs.lock_or_recover().insert(key, config);
}

/// Store an owner applier's last-applied buffer for a shared setup, so a guest
/// surface opens showing what the fixtures are actually doing.
pub fn receive_state<R: Runtime>(
    app: &AppHandle<R>,
    owner_sub: &str,
    setup_id: &str,
    payload: &[u8],
) {
    let guest = app.state::<LuxGuest>();
    if !guest.granted(owner_sub, setup_id) {
        return;
    }
    if payload.is_empty() {
        guest
            .states
            .lock_or_recover()
            .remove(&(owner_sub.to_owned(), setup_id.to_owned()));
        return;
    }
    let frame: lux_wire::ctl::Frame = match serde_json::from_slice(payload) {
        Ok(frame) => frame,
        Err(e) => {
            log::warn!("ignoring an unreadable state echo from {owner_sub}: {e}");
            return;
        }
    };
    if frame.version() != lux_wire::ctl::VERSION {
        return;
    }
    // Only a full buffer is a state echo; a channel frame here would be a
    // publisher doing something we don't model, not a partial update to merge.
    if let lux_wire::ctl::Frame::Buffer { buffer, .. } = frame {
        guest
            .states
            .lock_or_recover()
            .insert((owner_sub.to_owned(), setup_id.to_owned()), buffer);
    }
}

/// The topics a guest subscribes to for one grant: the owner's compiled setup
/// and their applier's state echo, and nothing else. Exactly the two the
/// authorizer grants receive on.
pub fn subscriptions(received: &[lux_wire::shares::ReceivedGrant]) -> Vec<String> {
    received
        .iter()
        .flat_map(|g| {
            [
                lux_wire::ctl::config_topic(&g.owner_sub, &g.setup_id),
                lux_wire::ctl::state_topic(&g.owner_sub, &g.setup_id),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(owner: &str, setup: &str) -> lux_wire::shares::ReceivedGrant {
        lux_wire::shares::ReceivedGrant {
            owner_sub: owner.into(),
            owner_label: format!("{owner}@example.com"),
            setup_id: setup.into(),
            setup_name: Some("Living room".into()),
            created_at: 0,
        }
    }

    #[test]
    fn subscriptions_are_the_two_topics_the_authorizer_grants() {
        let topics = subscriptions(&[grant("owner-1", "s-1")]);
        assert_eq!(
            topics,
            vec![
                "lux/ctl/user/owner-1/setup/s-1/config",
                "lux/ctl/user/owner-1/setup/s-1/state",
            ]
        );
        // Never the owner's frame topic: a guest publishes there, and receiving
        // it would mean seeing other guests' input.
        assert!(!topics.iter().any(|t| t.ends_with("/frame")));
        // Never a wildcard — a guest's reach is two exact topics per grant.
        assert!(!topics.iter().any(|t| t.contains('#') || t.contains('+')));

        let two = subscriptions(&[grant("owner-1", "s-1"), grant("owner-2", "s-9")]);
        assert_eq!(two.len(), 4);
    }
}
