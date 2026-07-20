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
//! **Writing is deliberately its own path.** A guest's controls publish to the
//! *owner's* frame topic and never touch this device's own buffer, and the
//! local input path never targets an owner's space. They share no state and no
//! function — moving a fader on someone else's desk must not move your own
//! fixtures, and moving your own must never reach into their rig. That
//! separation is structural rather than conditional, because the failure would
//! be silent and would be pointed at real lights.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use rumqttc::QoS;
use tauri::{AppHandle, Manager, Runtime};

use crate::lock::LockPolicy;

/// Trailing-edge coalescing window for a guest's control frames — the same
/// 40 ms the owner's own publishes use, so a drag costs the same ~25 Hz
/// whichever desk it is on.
const PUBLISH_WINDOW: Duration = Duration::from_millis(40);

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
    /// The shared desk this device currently has open, if any. Also the guard
    /// on every publish: with nothing open there is no target, so a stray call
    /// cannot reach anyone's rig.
    open: Mutex<Option<Key>>,
    /// Control writes accumulated between flushes. Separate from the local
    /// outbox on purpose (see the module docs) — they must never drain into
    /// each other's topic.
    outbox: Mutex<crate::nudge::Outbox>,
    publish_pending: AtomicBool,
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

/// Where a guest's control frames go: the open desk's owner's frame topic, or
/// nothing at all when no desk is open.
///
/// A pure function so the property that matters can be asserted directly — a
/// guest's writes land in the *owner's* namespace and never in this device's
/// own, whatever the surface above it does.
fn publish_target(open: Option<&Key>) -> Option<String> {
    open.map(|(owner_sub, setup_id)| lux_wire::ctl::frame_topic(owner_sub, setup_id))
}

/// Open a shared desk: remember it as the publish target, and announce this
/// guest on the owner's desk with a retained presence card.
///
/// The card goes on [`lux_wire::ctl::guest_presence_topic`] — one exact topic
/// per guest, which is what lets the authorizer grant it without a wildcard.
/// It is explicit publish/clear rather than a Last Will: a connection has one
/// will and it is already spoken for by this device's own presence, so an
/// ungraceful drop leaves the card until [`close_desk`] or the next sign-out.
/// The card carries a timestamp so a surface can render staleness.
pub fn open_desk<R: Runtime>(app: &AppHandle<R>, owner_sub: &str, setup_id: &str) {
    let guest = app.state::<LuxGuest>();
    if !guest.granted(owner_sub, setup_id) {
        log::warn!("refusing to open {owner_sub}/{setup_id}: no live grant");
        return;
    }
    // Leaving one desk for another clears the first card, so a guest is never
    // shown as live on two of an owner's setups at once.
    close_desk(app);
    *guest.open.lock_or_recover() = Some((owner_sub.to_owned(), setup_id.to_owned()));

    let Some(echo) = crate::nudge::connection(app) else {
        return;
    };
    let card = lux_wire::ctl::PresenceCard::new(
        echo.session.clone(),
        setup_id.to_owned(),
        crate::nudge::device_name(),
    );
    let topic = lux_wire::ctl::guest_presence_topic(owner_sub, &echo.sub);
    let Ok(payload) = serde_json::to_vec(&card) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(e) = echo
            .client
            .publish(topic.clone(), QoS::AtMostOnce, true, payload)
            .await
        {
            log::debug!("guest presence publish to {topic} failed: {e}");
        }
    });
}

/// Leave the open desk: stop publishing there and clear the presence card so
/// the owner's surface stops showing this guest as live.
pub fn close_desk<R: Runtime>(app: &AppHandle<R>) {
    let guest = app.state::<LuxGuest>();
    let Some((owner_sub, _)) = guest.open.lock_or_recover().take() else {
        return;
    };
    // Anything still queued was for the desk being left; it must not follow
    // the guest to the next one.
    guest.outbox.lock_or_recover().drain();
    let Some(echo) = crate::nudge::connection(app) else {
        return;
    };
    let topic = lux_wire::ctl::guest_presence_topic(&owner_sub, &echo.sub);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = echo
            .client
            .publish(topic.clone(), QoS::AtMostOnce, true, Vec::<u8>::new())
            .await
        {
            log::debug!("guest presence clear on {topic} failed: {e}");
        }
    });
}

/// Queue one slot write on the open shared desk.
pub fn publish_channel<R: Runtime>(app: &AppHandle<R>, ch: u16, val: u8) {
    let guest = app.state::<LuxGuest>();
    if guest.open.lock_or_recover().is_none() {
        return;
    }
    guest.outbox.lock_or_recover().push_channel(ch, val);
    schedule_flush(app);
}

/// Queue an overlay write (the colour-picker path) on the open shared desk.
pub fn publish_overlay<R: Runtime>(app: &AppHandle<R>, bytes: Vec<u8>) {
    let guest = app.state::<LuxGuest>();
    if guest.open.lock_or_recover().is_none() {
        return;
    }
    guest.outbox.lock_or_recover().push_overlay(bytes);
    schedule_flush(app);
}

/// Drain the guest outbox to the open desk's owner, trailing-edge coalesced.
/// Frames carry `src` so the owner's applier can attribute them in its per-slot
/// merge — and so this device drops its own frames when they echo back.
fn schedule_flush<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LuxGuest>();
    if state.publish_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PUBLISH_WINDOW).await;
        let guest = app.state::<LuxGuest>();
        guest.publish_pending.store(false, Ordering::SeqCst);

        // Re-read the target *after* the window: a guest who closed the desk
        // mid-drag must not have the tail of that drag delivered.
        let target = publish_target(guest.open.lock_or_recover().as_ref());
        let (Some(topic), Some(echo)) = (target, crate::nudge::connection(&app)) else {
            guest.outbox.lock_or_recover().drain();
            return;
        };
        let frames = guest.outbox.lock_or_recover().drain();
        for frame in frames {
            let Ok(payload) = serde_json::to_vec(&frame.with_src(&echo.session)) else {
                continue;
            };
            if let Err(e) = echo
                .client
                .publish(topic.clone(), QoS::AtMostOnce, false, payload)
                .await
            {
                log::debug!("guest ctl publish failed (connection likely down): {e}");
                return;
            }
        }
    });
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
    fn a_guests_writes_land_in_the_owners_namespace_and_never_its_own() {
        let me = "me-1";
        let open = ("owner-9".to_owned(), "s-1".to_owned());

        let topic = publish_target(Some(&open)).expect("an open desk has a target");
        assert_eq!(topic, "lux/ctl/user/owner-9/setup/s-1/frame");
        // The property worth pinning: a guest's control writes address the
        // owner's space. Publishing into our own would silently drive this
        // device's fixtures instead of the ones the user is looking at.
        assert!(!topic.contains(me));
        assert!(topic.starts_with(&lux_wire::ctl::user_prefix("owner-9")));

        // Nothing open, nothing to publish to — the guard that keeps a stray
        // call from reaching anyone's rig.
        assert_eq!(publish_target(None), None);
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
