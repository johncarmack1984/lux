//! The desktop↔sync-api wire contract, declared once.
//!
//! Both sides of the wire depend on this crate — `services/sync-api`
//! deserializes what `apps/desktop` serializes and vice versa — so the schema
//! cannot drift between them. (Before this crate each side hand-maintained its
//! own mirror of these shapes; that was the last unguarded wire in the app.)
//!
//! The JSON is byte-for-byte the pre-crate wire — camelCase keys, `Option`s
//! serialized as `null` — and the golden tests at the bottom pin every shape,
//! so editing a type here is a conscious, reviewable wire change rather than a
//! silent one. Old clients keep working: nothing here removes or renames a
//! field they rely on.
//!
//! [`nudge`] holds the other half of the spine: the tiny change-notification
//! frame and the MQTT topic scheme the sync-api publishes it on. [`ctl`] is
//! the remote-control channel riding the same connection: live buffer frames
//! between a user's own devices, parsed (unlike nudges) and versioned.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The path segment both sides build `…/setups[/{id}]` routes from.
pub const SETUPS_SEGMENT: &str = "setups";

/// Query parameter carrying the optimistic-concurrency base on `DELETE`.
pub const BASE_UPDATED_AT_QUERY: &str = "baseUpdatedAt";

/// The path segment for the caller's whole account: `DELETE /user` wipes every
/// item in their partition ahead of Cognito account deletion.
pub const USER_SEGMENT: &str = "user";

/// The path segment both sides build the `/settings` routes from.
pub const SETTINGS_SEGMENT: &str = "settings";

/// One setup as it crosses the wire (an element of [`ListSetupsResponse`]).
///
/// `fixtures` is deliberately opaque here: the server round-trips it as JSON
/// and only the desktop gives it a concrete type, so fixture-schema evolution
/// never requires a server deploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupRecord {
    pub id: String,
    pub name: String,
    pub universe: u16,
    pub fixtures: Value,
    /// Server-side write counter. Informational on the client today; the
    /// last-writer-wins authority is `updated_at`.
    pub rev: i64,
    /// Server-assigned epoch millis of the last write (never a client clock).
    pub updated_at: i64,
    /// Soft-delete tombstone; the client drops these during reconcile.
    #[serde(default)]
    pub deleted: bool,
}

/// Response to `GET /setups` — the whole pull in one request.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListSetupsResponse {
    pub setups: Vec<SetupRecord>,
    /// The account's settings record, riding the same partition query so a
    /// pull never needs a second round trip. `None` until the account's first
    /// settings push; also absent (defaulted) in replies from servers that
    /// predate settings, which old-shaped clients likewise ignore.
    #[serde(default)]
    pub settings: Option<SettingsRecord>,
}

/// Request body for `PUT /setups/{id}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertSetupBody {
    pub name: String,
    pub universe: u16,
    /// The desktop's `Vec<Fixture>`, opaque on the wire (see [`SetupRecord`]).
    pub fixtures: Value,
    /// The client's last-known server `updated_at` for this setup — the
    /// optimistic-concurrency token. `None` means "create; fail if it exists".
    #[serde(default)]
    pub base_updated_at: Option<i64>,
}

/// Response to a successful `PUT /setups/{id}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResponse {
    pub updated_at: i64,
    pub rev: i64,
}

/// Response to a successful `DELETE /setups/{id}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TombstoneResponse {
    pub updated_at: i64,
    pub deleted: bool,
}

/// The user's settings blob as it crosses the wire — one record per account,
/// last-writer-wins as a whole.
///
/// `data` is deliberately opaque here, exactly like [`SetupRecord::fixtures`]:
/// the server round-trips it as JSON and only the desktop gives it a concrete
/// type, so adding a new setting never requires a server deploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsRecord {
    pub data: Value,
    /// Server-side write counter. Informational on the client today; the
    /// last-writer-wins authority is `updated_at`.
    pub rev: i64,
    /// Server-assigned epoch millis of the last write (never a client clock).
    pub updated_at: i64,
}

/// Request body for `PUT /settings`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertSettingsBody {
    /// The desktop's settings blob, opaque on the wire (see [`SettingsRecord`]).
    pub data: Value,
    /// The client's last-known server `updated_at` for the settings record —
    /// the optimistic-concurrency token. `None` means "create; fail if it
    /// exists".
    #[serde(default)]
    pub base_updated_at: Option<i64>,
}

/// Response to a successful `DELETE /user` — the server-side data wipe that
/// precedes deleting the Cognito user (in-app account deletion).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteUserDataResponse {
    /// Items hard-deleted from the caller's partition.
    pub deleted_items: i64,
}

/// Error body for every non-2xx reply.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub mod nudge {
    //! The change-nudge channel: tiny events, opaque to clients.
    //!
    //! After each committed write the sync-api publishes
    //! [`setups_changed_frame`] to the writer's [`user_topic`]. The frame is
    //! deliberately content-free — **clients must not parse it**; any frame on
    //! the topic means "pull now", and the HTTP pull stays the authoritative
    //! sync. (This copies vegify's sync model, the house standard: the typed
    //! contract guards the pull payload, never the nudge.) Delivery is
    //! best-effort by design: a missed frame is healed by the existing
    //! pull-on-focus/reconnect safety nets.

    /// `{"changed":"setups"}` — published after a setup write. Opaque by
    /// design; see the module docs.
    pub fn setups_changed_frame() -> String {
        serde_json::json!({ "changed": "setups" }).to_string()
    }

    /// `{"changed":"settings"}` — published after a settings write. The label
    /// only aids log readability; clients treat every frame identically.
    pub fn settings_changed_frame() -> String {
        serde_json::json!({ "changed": "settings" }).to_string()
    }

    /// The per-user nudge topic. The IoT custom authorizer scopes each
    /// connection's policy to the *verified* Cognito `sub`, so a user can only
    /// ever subscribe to their own changes.
    pub fn user_topic(sub: &str) -> String {
        format!("lux/sync/user/{sub}")
    }

    /// MQTT client-id prefix for a user's nudge connections. Each app session
    /// appends a random suffix so one user's devices never kick each other off
    /// (IoT disconnects duplicate client ids); the authorizer allows
    /// `<prefix>*` for the verified sub only.
    pub fn client_id_prefix(sub: &str) -> String {
        format!("lux-sync-{sub}-")
    }

    /// Name of the handshake header (and query param) carrying the Cognito ID
    /// token — the IoT authorizer's `token_key_name`.
    pub const TOKEN_KEY: &str = "x-lux-token";

    /// Name of the IoT custom authorizer that gates nudge connections. Protocol
    /// naming (like the topic scheme), not environment config: every stack
    /// created from `infra/nudge.tf` registers its authorizer under exactly
    /// this name — the Terraform literal carries a cross-reference comment.
    pub const AUTHORIZER_NAME: &str = "lux-sync-auth";
}

pub mod ctl {
    //! The remote-control channel: live buffer writes between a user's own
    //! devices, riding the same standing IoT connection as [`super::nudge`].
    //!
    //! Topic scheme, all under the per-user prefix the authorizer's policy
    //! wildcards ([`user_prefix`]):
    //!
    //! - `…/setup/<setupId>/frame` — live control frames, QoS 0, not retained.
    //! - `…/setup/<setupId>/state` — retained echo of the applier's last-applied
    //!   full buffer (a [`Frame::Buffer`]), so every surface can reflect truth.
    //! - `…/presence/<session>` — retained presence card; an **empty retained
    //!   payload clears it** and is also the connection's Last Will. Presence is
    //!   session-scoped, not setup-scoped, because a connection has exactly one
    //!   will and the active setup changes without reconnecting — the card's
    //!   `setupId` carries the binding.
    //! - `…/setup/<setupId>/config` — reserved for render-node compiled setups.
    //!
    //! Unlike nudge frames (opaque by contract), ctl payloads are parsed —
    //! listeners route by topic. Every payload carries `v` from day one: the
    //! desktop updates within hours while iOS waits on App Store review, so
    //! version skew between one user's own devices is the normal case, and
    //! readers drop frames whose version they don't know. Shapes stay flat and
    //! small on purpose — an embedded render node must be able to consume
    //! `frame`/`config` with a fixed-size parser.

    use serde::{Deserialize, Serialize};

    /// Current ctl payload version, stamped by the constructors. Readers drop
    /// payloads with any other version (log + ignore, never an error surface).
    pub const VERSION: u32 = 1;

    /// The per-user ctl namespace, no trailing slash: `lux/ctl/user/<sub>`.
    /// The authorizer grants publish/subscribe/receive on `<prefix>/*`.
    pub fn user_prefix(sub: &str) -> String {
        format!("lux/ctl/user/{sub}")
    }

    /// The subscription filter covering a user's whole ctl space.
    pub fn user_filter(sub: &str) -> String {
        format!("{}/#", user_prefix(sub))
    }

    /// Live control frames for one setup (remote surface → applier).
    pub fn frame_topic(sub: &str, setup_id: &str) -> String {
        format!("{}/setup/{setup_id}/frame", user_prefix(sub))
    }

    /// Retained last-applied buffer echo for one setup (applier → surfaces).
    pub fn state_topic(sub: &str, setup_id: &str) -> String {
        format!("{}/setup/{setup_id}/state", user_prefix(sub))
    }

    /// Reserved: retained compiled setup for render nodes. No publisher yet;
    /// named now so the scheme is carved once.
    pub fn config_topic(sub: &str, setup_id: &str) -> String {
        format!("{}/setup/{setup_id}/config", user_prefix(sub))
    }

    /// Retained presence card for one connection (`session` = the random
    /// client-id suffix the connection already mints).
    pub fn presence_topic(sub: &str, session: &str) -> String {
        format!("{}/presence/{session}", user_prefix(sub))
    }

    /// One live control write. The two kinds mirror the command layer's two
    /// buffer mutations, so concurrent editors compose instead of clobbering
    /// each other with stale full snapshots: a fader drag touches one slot, a
    /// color-pick overlays the leading slots, and cross-device races resolve
    /// per-slot last-write-wins at the applier.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum Frame {
        /// `{"v":1,"buffer":[…]}` — overlay onto the leading slots, exactly
        /// `LuxBuffer::set` semantics (higher slots untouched).
        Buffer { v: u32, buffer: Vec<u8> },
        /// `{"v":1,"ch":10,"val":200}` — one slot; `ch` is the 1-based DMX
        /// slot number, matching `set_channel`.
        Channel { v: u32, ch: u16, val: u8 },
    }

    impl Frame {
        pub fn buffer(buffer: Vec<u8>) -> Self {
            Frame::Buffer { v: VERSION, buffer }
        }

        pub fn channel(ch: u16, val: u8) -> Self {
            Frame::Channel {
                v: VERSION,
                ch,
                val,
            }
        }

        /// The payload's wire version — gate on this before applying.
        pub fn version(&self) -> u32 {
            match self {
                Frame::Buffer { v, .. } | Frame::Channel { v, .. } => *v,
            }
        }
    }

    /// Retained presence card: "this connection is live, applying `setup_id`".
    /// Republished on active-setup change; cleared (empty retained payload) on
    /// sign-out/shutdown and by the connection's Last Will on ungraceful drops.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PresenceCard {
        pub v: u32,
        /// The connection's session id (its client-id suffix) — matches the
        /// topic segment, so a card is self-describing off-topic too.
        pub session: String,
        /// The setup this peer currently has active and applies frames for.
        pub setup_id: String,
        /// Human-readable device name, so surfaces (and receiver tooling) can
        /// say *which* device is live.
        pub name: String,
    }

    impl PresenceCard {
        pub fn new(session: String, setup_id: String, name: String) -> Self {
            Self {
                v: VERSION,
                session,
                setup_id,
                name,
            }
        }
    }
}

// --- golden tests: the wire's own drift gate ---------------------------------
//
// Each test pins the exact JSON a type produces/accepts. If one of these fails,
// you are changing the wire — make sure every deployed client and the Lambda
// agree before you ship it, then update the golden here in the same commit.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn setup_record_shape() {
        let record = SetupRecord {
            id: "7f0175a6-3b64-4a2a-9e1c-000000000001".into(),
            name: "Home".into(),
            universe: 1,
            fixtures: json!([{ "kind": "rgbaw" }]),
            rev: 3,
            updated_at: 1719000000000,
            deleted: false,
        };
        assert_eq!(
            serde_json::to_string(&record).unwrap(),
            r#"{"id":"7f0175a6-3b64-4a2a-9e1c-000000000001","name":"Home","universe":1,"fixtures":[{"kind":"rgbaw"}],"rev":3,"updatedAt":1719000000000,"deleted":false}"#
        );
    }

    #[test]
    fn setup_record_tolerates_missing_deleted() {
        let record: SetupRecord = serde_json::from_value(json!({
            "id": "a", "name": "n", "universe": 1, "fixtures": [], "rev": 0,
            "updatedAt": 1
        }))
        .unwrap();
        assert!(!record.deleted);
    }

    #[test]
    fn list_response_shape() {
        let body = ListSetupsResponse {
            setups: vec![],
            settings: None,
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"setups":[],"settings":null}"#
        );

        // A reply from a server that predates settings still parses.
        let old: ListSetupsResponse = serde_json::from_str(r#"{"setups":[]}"#).unwrap();
        assert!(old.settings.is_none());

        let with: ListSetupsResponse = serde_json::from_str(
            r#"{"setups":[],"settings":{"data":{"sliderOrientation":"horizontal"},"rev":1,"updatedAt":42}}"#,
        )
        .unwrap();
        let record = with.settings.unwrap();
        assert_eq!(record.updated_at, 42);
        assert_eq!(record.data["sliderOrientation"], "horizontal");
    }

    #[test]
    fn upsert_body_shape() {
        // `baseUpdatedAt: null` (not omitted) is what shipped clients send —
        // pinned here on purpose.
        let create = UpsertSetupBody {
            name: "Home".into(),
            universe: 7,
            fixtures: json!([]),
            base_updated_at: None,
        };
        assert_eq!(
            serde_json::to_string(&create).unwrap(),
            r#"{"name":"Home","universe":7,"fixtures":[],"baseUpdatedAt":null}"#
        );

        let update: UpsertSetupBody = serde_json::from_str(
            r#"{"name":"Home","universe":7,"fixtures":[],"baseUpdatedAt":42}"#,
        )
        .unwrap();
        assert_eq!(update.base_updated_at, Some(42));

        // A body without the field at all (very old client) still parses.
        let bare: UpsertSetupBody =
            serde_json::from_str(r#"{"name":"Home","universe":7,"fixtures":[]}"#).unwrap();
        assert_eq!(bare.base_updated_at, None);
    }

    #[test]
    fn write_response_shape() {
        let body = WriteResponse {
            updated_at: 42,
            rev: 2,
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"updatedAt":42,"rev":2}"#
        );
    }

    #[test]
    fn tombstone_response_shape() {
        let body = TombstoneResponse {
            updated_at: 42,
            deleted: true,
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"updatedAt":42,"deleted":true}"#
        );
    }

    #[test]
    fn settings_record_shape() {
        let record = SettingsRecord {
            data: json!({ "sliderOrientation": "vertical" }),
            rev: 2,
            updated_at: 1719000000000,
        };
        assert_eq!(
            serde_json::to_string(&record).unwrap(),
            r#"{"data":{"sliderOrientation":"vertical"},"rev":2,"updatedAt":1719000000000}"#
        );
    }

    #[test]
    fn upsert_settings_body_shape() {
        // `baseUpdatedAt: null` (not omitted) mirrors the setups body — pinned.
        let create = UpsertSettingsBody {
            data: json!({ "sliderOrientation": "vertical" }),
            base_updated_at: None,
        };
        assert_eq!(
            serde_json::to_string(&create).unwrap(),
            r#"{"data":{"sliderOrientation":"vertical"},"baseUpdatedAt":null}"#
        );

        let update: UpsertSettingsBody =
            serde_json::from_str(r#"{"data":{},"baseUpdatedAt":42}"#).unwrap();
        assert_eq!(update.base_updated_at, Some(42));

        // A body without the field at all still parses.
        let bare: UpsertSettingsBody = serde_json::from_str(r#"{"data":{}}"#).unwrap();
        assert_eq!(bare.base_updated_at, None);
    }

    #[test]
    fn delete_user_data_response_shape() {
        let body = DeleteUserDataResponse { deleted_items: 3 };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"deletedItems":3}"#
        );
    }

    #[test]
    fn error_response_shape() {
        let body = ErrorResponse {
            error: "conflict".into(),
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"error":"conflict"}"#
        );
    }

    #[test]
    fn nudge_frame_and_topics() {
        assert_eq!(nudge::setups_changed_frame(), r#"{"changed":"setups"}"#);
        assert_eq!(nudge::settings_changed_frame(), r#"{"changed":"settings"}"#);
        assert_eq!(nudge::user_topic("abc-123"), "lux/sync/user/abc-123");
        assert_eq!(nudge::client_id_prefix("abc-123"), "lux-sync-abc-123-");
    }

    #[test]
    fn ctl_topics() {
        assert_eq!(ctl::user_prefix("abc-123"), "lux/ctl/user/abc-123");
        assert_eq!(ctl::user_filter("abc-123"), "lux/ctl/user/abc-123/#");
        assert_eq!(
            ctl::frame_topic("abc-123", "s-1"),
            "lux/ctl/user/abc-123/setup/s-1/frame"
        );
        assert_eq!(
            ctl::state_topic("abc-123", "s-1"),
            "lux/ctl/user/abc-123/setup/s-1/state"
        );
        assert_eq!(
            ctl::config_topic("abc-123", "s-1"),
            "lux/ctl/user/abc-123/setup/s-1/config"
        );
        assert_eq!(
            ctl::presence_topic("abc-123", "0a1b2c3d"),
            "lux/ctl/user/abc-123/presence/0a1b2c3d"
        );
    }

    #[test]
    fn ctl_frame_shapes() {
        // Overlay frame: the color-picker path, LuxBuffer::set semantics.
        let overlay = ctl::Frame::buffer(vec![121, 255, 0]);
        assert_eq!(
            serde_json::to_string(&overlay).unwrap(),
            r#"{"v":1,"buffer":[121,255,0]}"#
        );

        // Channel frame: the fader path, 1-based DMX slot.
        let channel = ctl::Frame::channel(10, 200);
        assert_eq!(
            serde_json::to_string(&channel).unwrap(),
            r#"{"v":1,"ch":10,"val":200}"#
        );

        // Parsing picks the right kind from the fields alone (untagged).
        let parsed: ctl::Frame = serde_json::from_str(r#"{"v":1,"buffer":[1,2]}"#).unwrap();
        assert_eq!(parsed, ctl::Frame::buffer(vec![1, 2]));
        let parsed: ctl::Frame = serde_json::from_str(r#"{"v":1,"ch":512,"val":0}"#).unwrap();
        assert_eq!(parsed, ctl::Frame::channel(512, 0));
        assert_eq!(parsed.version(), 1);

        // A future shape that matches neither kind fails to parse — readers
        // treat that exactly like an unknown version: log + drop.
        assert!(serde_json::from_str::<ctl::Frame>(r#"{"v":2,"verb":"fade"}"#).is_err());

        // A known kind with a newer version still parses; the version gate is
        // the reader's job, so it must survive deserialization.
        let future: ctl::Frame = serde_json::from_str(r#"{"v":9,"ch":1,"val":1}"#).unwrap();
        assert_eq!(future.version(), 9);
    }

    #[test]
    fn ctl_presence_card_shape() {
        let card = ctl::PresenceCard::new("0a1b2c3d".into(), "s-1".into(), "Mac Mini".into());
        assert_eq!(
            serde_json::to_string(&card).unwrap(),
            r#"{"v":1,"session":"0a1b2c3d","setupId":"s-1","name":"Mac Mini"}"#
        );

        let parsed: ctl::PresenceCard = serde_json::from_str(
            r#"{"v":1,"session":"0a1b2c3d","setupId":"s-1","name":"Mac Mini"}"#,
        )
        .unwrap();
        assert_eq!(parsed, card);
    }
}
