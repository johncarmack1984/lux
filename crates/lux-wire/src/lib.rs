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
//! frame and the MQTT topic scheme the sync-api publishes it on.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The path segment both sides build `…/setups[/{id}]` routes from.
pub const SETUPS_SEGMENT: &str = "setups";

/// Query parameter carrying the optimistic-concurrency base on `DELETE`.
pub const BASE_UPDATED_AT_QUERY: &str = "baseUpdatedAt";

/// The path segment for the caller's whole account: `DELETE /user` wipes every
/// item in their partition ahead of Cognito account deletion.
pub const USER_SEGMENT: &str = "user";

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

/// Response to `GET /setups`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ListSetupsResponse {
    pub setups: Vec<SetupRecord>,
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

    /// `{"changed":"setups"}` — the only frame currently published. Opaque by
    /// design; see the module docs.
    pub fn setups_changed_frame() -> String {
        serde_json::json!({ "changed": "setups" }).to_string()
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
        let body = ListSetupsResponse { setups: vec![] };
        assert_eq!(serde_json::to_string(&body).unwrap(), r#"{"setups":[]}"#);
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
        assert_eq!(nudge::user_topic("abc-123"), "lux/sync/user/abc-123");
        assert_eq!(nudge::client_id_prefix("abc-123"), "lux-sync-abc-123-");
    }
}
