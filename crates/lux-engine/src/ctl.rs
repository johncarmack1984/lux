//! Remote-control decisions for the user channel: where an incoming publish
//! goes, and whether a ctl frame should be applied. Plain functions over
//! plain types on purpose — the desktop's Tauri listener and the headless
//! node share them unchanged.

/// Where an incoming publish on the user channel goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Route<'t> {
    /// The opaque nudge topic — schedule a pull (peers only; the node ignores it).
    Nudge,
    /// A live ctl frame addressed to one setup.
    Frame { setup_id: &'t str },
    /// An applier's retained state echo for one setup — reflected, never applied.
    State { setup_id: &'t str },
    /// A peer's retained presence card (empty payload = the peer is gone).
    Presence { session: &'t str },
    /// Reserved render-node config traffic.
    Config,
    /// Not a topic this listener expects under the granted policy.
    Unknown,
}

/// Classify an incoming topic for the user identified by `sub`.
pub fn route<'t>(topic: &'t str, sub: &str) -> Route<'t> {
    if topic == lux_wire::nudge::user_topic(sub) {
        return Route::Nudge;
    }
    let prefix = lux_wire::ctl::user_prefix(sub);
    let Some(rest) = topic
        .strip_prefix(prefix.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return Route::Unknown;
    };
    if let Some(setup_rest) = rest.strip_prefix("setup/") {
        return match setup_rest.split_once('/') {
            Some((setup_id, "frame")) => Route::Frame { setup_id },
            Some((setup_id, "state")) => Route::State { setup_id },
            Some((_, "config")) => Route::Config,
            _ => Route::Unknown,
        };
    }
    if let Some(session) = rest.strip_prefix("presence/") {
        return Route::Presence { session };
    }
    Route::Unknown
}

/// An incoming publish from *another* account's ctl space — one this
/// connection holds a shared-control grant in (docs/shared-control.md).
///
/// Separate from [`Route`] rather than folded into it because the two answer
/// different questions. [`Route`] asks "what is this in my own space", where
/// the sub is known up front. This asks "which owner and setup is this about",
/// where the owner is what the topic is telling us. A guest also sees strictly
/// less: frames and presence in an owner's space are not its business, and the
/// authorizer never grants receive on them.
#[derive(Debug, PartialEq, Eq)]
pub enum GuestRoute<'t> {
    /// The owner's compiled setup — a guest surface's entire source of truth
    /// for what the desk looks like. An empty payload means the grant ended or
    /// the setup is gone.
    Config {
        owner_sub: &'t str,
        setup_id: &'t str,
    },
    /// The owner applier's retained state echo — what the fixtures are actually
    /// doing, so a guest's faders open at the truth rather than at zero.
    State {
        owner_sub: &'t str,
        setup_id: &'t str,
    },
}

/// Classify a publish arriving from an owner's space. `own_sub` is this
/// connection's own user, whose traffic belongs to [`route`] instead — so the
/// two classifiers never both claim a topic.
///
/// Structural only: it reports what a topic *says*, not whether the grant
/// behind it is live. The authorizer already decides what may be received, and
/// the caller matches the pair against its own list of grants before acting.
pub fn guest_route<'t>(topic: &'t str, own_sub: &str) -> Option<GuestRoute<'t>> {
    let rest = topic.strip_prefix("lux/ctl/user/")?;
    let (owner_sub, rest) = rest.split_once('/')?;
    if owner_sub.is_empty() || owner_sub == own_sub {
        return None; // our own space
    }
    let (setup_id, leaf) = rest.strip_prefix("setup/")?.split_once('/')?;
    if setup_id.is_empty() {
        return None;
    }
    match leaf {
        "config" => Some(GuestRoute::Config {
            owner_sub,
            setup_id,
        }),
        "state" => Some(GuestRoute::State {
            owner_sub,
            setup_id,
        }),
        _ => None,
    }
}

/// One applicable buffer mutation extracted from a gated ctl frame.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoteApply {
    Overlay(Vec<u8>),
    Channel { ch: u16, val: u8 },
}

/// Whether this peer applies `frame`: the version must be known, the frame
/// must not be this connection's own publish echoed back (`src` == our
/// session), and only frames addressed to the active setup apply.
pub fn gate(
    frame: lux_wire::ctl::Frame,
    frame_setup: &str,
    active_setup: &str,
    own_session: &str,
) -> Option<RemoteApply> {
    if frame.version() != lux_wire::ctl::VERSION {
        log::debug!(
            "dropping ctl frame with unknown version {}",
            frame.version()
        );
        return None;
    }
    if frame.src() == Some(own_session) {
        return None; // our own frame, already applied locally
    }
    if frame_setup != active_setup {
        log::debug!("dropping ctl frame for inactive setup {frame_setup}");
        return None;
    }
    match frame {
        lux_wire::ctl::Frame::Buffer { buffer, .. } => Some(RemoteApply::Overlay(buffer)),
        lux_wire::ctl::Frame::Channel { ch, val, .. } => Some(RemoteApply::Channel { ch, val }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lux_wire::ctl::Frame;

    #[test]
    fn guest_route_reads_the_owner_and_setup_off_the_topic() {
        let me = "me-1";
        assert_eq!(
            guest_route("lux/ctl/user/owner-9/setup/s-1/config", me),
            Some(GuestRoute::Config {
                owner_sub: "owner-9",
                setup_id: "s-1"
            })
        );
        assert_eq!(
            guest_route("lux/ctl/user/owner-9/setup/s-1/state", me),
            Some(GuestRoute::State {
                owner_sub: "owner-9",
                setup_id: "s-1"
            })
        );

        // Our own space belongs to `route`; the two never both claim a topic.
        assert_eq!(guest_route("lux/ctl/user/me-1/setup/s-1/config", me), None);
        assert_eq!(route("lux/ctl/user/me-1/setup/s-1/state", me), Route::State { setup_id: "s-1" });

        // A guest is never granted receive on these, and must not act on one
        // if a future policy change ever delivered it.
        assert_eq!(guest_route("lux/ctl/user/owner-9/setup/s-1/frame", me), None);
        assert_eq!(guest_route("lux/ctl/user/owner-9/presence/me-1", me), None);

        // Malformed or foreign topics are not guessed at.
        assert_eq!(guest_route("lux/sync/user/owner-9", me), None);
        assert_eq!(guest_route("lux/ctl/user//setup/s-1/config", me), None);
        assert_eq!(guest_route("lux/ctl/user/owner-9/setup//config", me), None);
        assert_eq!(guest_route("lux/ctl/user/owner-9/setup/s-1", me), None);
        assert_eq!(guest_route("nonsense", me), None);
    }

    #[test]
    fn route_classifies_the_user_channel() {
        let sub = "abc-123";
        assert_eq!(route("lux/sync/user/abc-123", sub), Route::Nudge);
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/frame", sub),
            Route::Frame { setup_id: "s-1" }
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/state", sub),
            Route::State { setup_id: "s-1" }
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/config", sub),
            Route::Config
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/presence/0a1b2c3d", sub),
            Route::Presence {
                session: "0a1b2c3d"
            }
        );

        // Not ours / not a shape we know.
        assert_eq!(route("lux/sync/user/other", sub), Route::Unknown);
        assert_eq!(
            route("lux/ctl/user/other/setup/s-1/frame", sub),
            Route::Unknown
        );
        assert_eq!(
            route("lux/ctl/user/abc-123/setup/s-1/verbs", sub),
            Route::Unknown
        );
        assert_eq!(route("lux/ctl/user/abc-123/setup/s-1", sub), Route::Unknown);
        assert_eq!(route("lux/ctl/user/abc-123", sub), Route::Unknown);
        assert_eq!(route("lux/1/buffer/set", sub), Route::Unknown);
    }

    #[test]
    fn gate_applies_only_known_versions_for_the_active_setup() {
        let overlay = Frame::buffer(vec![1, 2, 3]);
        assert_eq!(
            gate(overlay, "s-1", "s-1", "me00"),
            Some(RemoteApply::Overlay(vec![1, 2, 3]))
        );
        let channel = Frame::channel(10, 200);
        assert_eq!(
            gate(channel, "s-1", "s-1", "me00"),
            Some(RemoteApply::Channel { ch: 10, val: 200 })
        );

        // Inactive setup → dropped.
        assert_eq!(gate(Frame::channel(1, 1), "s-2", "s-1", "me00"), None);

        // Unknown version → dropped (parse it as the reader would).
        let future: Frame = serde_json::from_str(r#"{"v":9,"ch":1,"val":1}"#).expect("parses");
        assert_eq!(gate(future, "s-1", "s-1", "me00"), None);
    }

    #[test]
    fn gate_drops_our_own_frames_but_applies_other_sessions() {
        let own = Frame::channel(1, 255).with_src("me00");
        assert_eq!(gate(own, "s-1", "s-1", "me00"), None);

        let theirs = Frame::channel(1, 255).with_src("them");
        assert!(gate(theirs, "s-1", "s-1", "me00").is_some());

        // Unstamped (e.g. CLI-published) frames apply.
        assert!(gate(Frame::channel(1, 255), "s-1", "s-1", "me00").is_some());
    }
}
