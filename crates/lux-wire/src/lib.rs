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

pub mod apple {
    //! Sign in with Apple: desktop ↔ `lux-apple-auth` wire.
    //!
    //! The desktop runs the native `ASAuthorizationController` sheet, then
    //! posts the resulting Apple identity token here. The service verifies it
    //! against Apple's JWKS (audience = the app's bundle id, nonce bound to
    //! [`SignInRequest::raw_nonce`]), maps Apple's stable `sub` to a Cognito
    //! user, and mints normal user-pool tokens through the pool's custom-auth
    //! triggers — so everything downstream of sign-in sees the same JWTs the
    //! SRP path produces.
    //!
    //! Routes (all `POST`, on the service's own Function URL):
    //! - `/auth/apple`        — sign in (creating or linking a user on first use)
    //! - `/auth/apple/link`   — bearer-authed: link the CALLER's account to the
    //!   presented Apple credential, regardless of email (the Hide-My-Email path)
    //! - `/auth/apple/revoke` — bearer-authed: revoke the stored Apple token and
    //!   drop the link (account deletion runs this before wiping data)

    use serde::{Deserialize, Serialize};

    /// Path segments the service routes on: `/auth/apple[/link|/revoke]`.
    pub const AUTH_SEGMENT: &str = "auth";
    pub const APPLE_SEGMENT: &str = "apple";
    pub const LINK_SEGMENT: &str = "link";
    pub const REVOKE_SEGMENT: &str = "revoke";

    /// Body for `POST /auth/apple` and `POST /auth/apple/link`.
    ///
    /// `email` and `full_name` ride along because **Apple surfaces them only on
    /// the first authorization** — the service persists them then or they are
    /// gone (the user can reset the grant in Settings). They are display/record
    /// data only; the email that drives account linking always comes from the
    /// verified token, never the body.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SignInRequest {
        /// Apple's identity token (a JWT) from the authorization sheet.
        pub identity_token: String,
        /// The single-use, short-lived authorization code from the same sheet;
        /// the service exchanges it for the revocable Apple refresh token that
        /// account deletion is required to revoke.
        pub authorization_code: String,
        /// The raw nonce whose SHA-256 the client set on the sheet request; the
        /// service re-hashes it and requires the token's `nonce` claim to match.
        pub raw_nonce: String,
        #[serde(default)]
        pub email: Option<String>,
        #[serde(default)]
        pub full_name: Option<String>,
    }

    /// Response to a successful `POST /auth/apple` — Cognito user-pool tokens,
    /// stored by the desktop exactly like an SRP sign-in's.
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SignInResponse {
        pub id_token: String,
        pub access_token: String,
        pub refresh_token: String,
        /// Access/id token lifetime in seconds, from Cognito.
        pub expires_in: i32,
        /// True when this sign-in created a brand-new account (vs. signing into
        /// or auto-linking an existing one) — the UI's "welcome" cue.
        #[serde(default)]
        pub created: bool,
    }

    /// Response to a successful `POST /auth/apple/link`.
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LinkResponse {
        pub linked: bool,
    }

    /// Response to a successful `POST /auth/apple/revoke`.
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RevokeResponse {
        pub revoked: bool,
    }
}

pub mod device {
    //! Headless device pairing: lux-node ↔ auth service wire
    //! (docs/claim-code-pairing.md).
    //!
    //! An unpaired node registers and polls over plain HTTPS; the owner
    //! approves from the app, which sees only pending devices that share its
    //! NAT egress (same public IP). Approval binds the device to the owner's
    //! account and a setup; the node's next poll returns an ordinary Cognito
    //! refresh token minted on the device app client.
    //!
    //! Routes (on the auth service's Function URL):
    //! - `POST /auth/device/authorize` — device: register, get a code pair
    //! - `POST /auth/device/token`     — device: poll for the grant
    //! - `GET  /auth/device/pending`   — bearer-authed: same-egress pending list
    //! - `POST /auth/device/approve`   — bearer-authed: approve one device
    //! - `GET  /auth/device/list`      — bearer-authed: the owner's paired devices
    //! - `POST /auth/device/revoke`    — bearer-authed: remove a paired device

    use serde::{Deserialize, Serialize};

    /// Path segments under [`super::apple::AUTH_SEGMENT`]:
    /// `/auth/device/{authorize|token|pending|approve|list|revoke}`.
    pub const DEVICE_SEGMENT: &str = "device";
    pub const AUTHORIZE_SEGMENT: &str = "authorize";
    pub const TOKEN_SEGMENT: &str = "token";
    pub const PENDING_SEGMENT: &str = "pending";
    pub const APPROVE_SEGMENT: &str = "approve";
    pub const LIST_SEGMENT: &str = "list";
    pub const REVOKE_SEGMENT: &str = "revoke";

    /// Body for `POST /auth/device/authorize`. Everything here is display
    /// metadata for the approve screen — identity is established by approval,
    /// never claimed by the device.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AuthorizeRequest {
        /// The node's stable self-generated id (uuid, persisted in its state
        /// dir). Lets a re-registering node supersede its own earlier codes.
        pub device_id: String,
        pub hostname: String,
        /// Last 4 hex digits of the primary MAC — matches the sticker/port
        /// label, the approve screen's physical cross-check.
        pub mac_tail: String,
        pub version: String,
        pub arch: String,
    }

    /// Response to `POST /auth/device/authorize` (RFC 8628 §3.2 shape).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AuthorizeResponse {
        /// The device's secret — polls `/token` with it. Never shown to humans.
        pub device_code: String,
        /// Short display code (`LUX-XXXX`) — shown in the app and the node's
        /// journal for the human cross-check. Never typed anywhere.
        pub user_code: String,
        /// Seconds between `/token` polls.
        pub interval: u32,
        /// Seconds until this code pair expires and the node re-registers.
        pub expires_in: u32,
    }

    /// Body for `POST /auth/device/token`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TokenRequest {
        pub device_code: String,
    }

    /// Response to `POST /auth/device/token`. `status` follows RFC 8628 §3.5
    /// (`authorization_pending`, `slow_down`, `expired_token`, `access_denied`)
    /// plus `granted`, which carries the session fields.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TokenResponse {
        pub status: String,
        /// On `granted`: what the node writes into session.json (the account's
        /// email attribute — usernames are UUIDs for Apple-created users) …
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub email: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub refresh_token: Option<String>,
        /// The app client that minted the refresh token — the node refreshes
        /// against this, never the interactive client.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub client_id: Option<String>,
        /// … and the setup binding the approver chose.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub setup_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub universe: Option<u16>,
    }

    /// One pending device on the approve screen (`GET /auth/device/pending`).
    /// `pair_ref` is an opaque handle for `/approve` — the app never sees the
    /// device code itself.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PendingDevice {
        pub pair_ref: String,
        pub user_code: String,
        pub hostname: String,
        pub mac_tail: String,
        pub version: String,
        pub arch: String,
        /// Epoch millis of the registration (server clock).
        pub first_seen: i64,
    }

    /// Response to `GET /auth/device/pending`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PendingResponse {
        pub devices: Vec<PendingDevice>,
    }

    /// Body for `POST /auth/device/approve`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ApproveRequest {
        /// [`PendingDevice::pair_ref`] from the pending list.
        pub pair_ref: String,
        /// The setup this node will drive (the picker replaces `lux-node
        /// install`'s interactive list for appliances).
        pub setup_id: String,
        /// sACN universe; defaults to 1.
        #[serde(default)]
        pub universe: Option<u16>,
        /// Display name for the device registry; defaults to the hostname.
        #[serde(default)]
        pub name: Option<String>,
    }

    /// Response to a successful `POST /auth/device/approve`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ApproveResponse {
        pub approved: bool,
    }

    /// One paired device in the owner's registry (`GET /auth/device/list`) —
    /// the app's device list, and the account-deletion confirm's tally.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DeviceRecord {
        pub device_id: String,
        pub name: String,
        pub hostname: String,
        pub setup_id: String,
        pub universe: u16,
        /// Epoch millis (server clock).
        pub paired_at: i64,
    }

    /// Response to `GET /auth/device/list`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ListResponse {
        pub devices: Vec<DeviceRecord>,
    }

    /// Body for `POST /auth/device/revoke` — bearer-authed: the owner removes
    /// one of their paired devices. v1 is data-plane only — the device drops
    /// out of [`ListResponse`] at once; cutting the box's live IoT access is
    /// authorizer-level enforcement, a deliberately deferred design
    /// open-question (docs/claim-code-pairing.md §Revocation).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RevokeRequest {
        /// [`DeviceRecord::device_id`] of the device to remove.
        pub device_id: String,
    }

    /// Response to a `POST /auth/device/revoke`. `revoked` is `false` only when
    /// the caller owns no such device (an idempotent no-op, not an error).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RevokeResponse {
        pub revoked: bool,
    }
}

pub mod shares {
    //! Shared control: granting one contact the desk for one setup
    //! (docs/shared-control.md).
    //!
    //! A grant is a triple — (owner, contact, setup). Nothing here shares an
    //! *account*: the contact signs in as themselves, and the grant only widens
    //! what their own identity may do on the owner's ctl space (publish frames
    //! and a presence card; subscribe the applier's state and config). The
    //! owner's applier stays the sole DMX authority, and guests never sync:
    //! their surface renders from the retained [`super::ctl::Config`] alone.
    //!
    //! Invites are claim codes, not emails — the owner mints a short-lived
    //! single-use code and sends it over their own channel (iMessage), so there
    //! is no mail infrastructure and no deliverability surface, and contacts
    //! using Hide My Email work by construction.
    //!
    //! Routes (on the sync API's Function URL, all bearer-authed):
    //! - `POST   /shares/invite`                        — owner: mint a code
    //! - `POST   /shares/claim`                         — contact: redeem one
    //! - `GET    /shares`                               — both directions at once
    //! - `DELETE /shares/granted/{contactSub}/{setupId}` — owner: revoke
    //! - `DELETE /shares/received/{ownerSub}/{setupId}`  — contact: leave
    //! - `DELETE /shares/invite/{codeRef}`               — owner: withdraw a code

    use serde::{Deserialize, Serialize};

    /// Path segments the sync API routes on (see the module docs).
    pub const SHARES_SEGMENT: &str = "shares";
    pub const INVITE_SEGMENT: &str = "invite";
    pub const CLAIM_SEGMENT: &str = "claim";
    pub const GRANTED_SEGMENT: &str = "granted";
    pub const RECEIVED_SEGMENT: &str = "received";

    /// Hard ceiling on the live grants **one contact** may hold, across all
    /// owners. This is not a product limit dressed up as a safety limit: an IoT
    /// custom authorizer may return at most 10 policy documents, the contact's
    /// own space takes one, and each grant needs its own (a grant's six ARNs
    /// run ~1 KB against the 2048-character per-document ceiling, so two grants
    /// will not reliably share a document). Past this the authorizer would have
    /// to silently drop grants; instead the claim route refuses loudly and the
    /// authorizer truncates with an error log if it ever sees more.
    pub const MAX_GRANTS_PER_CONTACT: usize = 9;

    /// Outstanding (unclaimed, unexpired) invites one owner may hold at once.
    /// Codes are bearer credentials, so a bounded number of them is a smaller
    /// standing liability — and it keeps the pending list an honest UI.
    pub const MAX_PENDING_INVITES: usize = 10;

    /// Invite lifetime. Long enough to survive "I'll set this up tonight",
    /// short enough that a screenshotted code in a chat log goes stale.
    pub const INVITE_TTL_SECS: i64 = 48 * 60 * 60;

    /// Body for `POST /shares/invite`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct InviteRequest {
        /// The setup this code will grant. Must be one of the caller's own.
        pub setup_id: String,
        /// The owner's private note for their manage list ("Chelsea"). Never
        /// shown to the contact — it is a label *for* them, not *by* them.
        #[serde(default)]
        pub label: Option<String>,
    }

    /// Response to `POST /shares/invite`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct InviteResponse {
        /// The claim code, formatted for a human to send in a message. This is
        /// the only time the server can produce it — only its hash is stored.
        pub code: String,
        /// Opaque handle for `DELETE /shares/invite/{codeRef}`, so the minting
        /// app can withdraw the code without waiting for a list refresh.
        pub code_ref: String,
        /// Epoch millis (server clock).
        pub expires_at: i64,
    }

    /// Body for `POST /shares/claim`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ClaimRequest {
        /// As typed or pasted; the server normalizes case and separators.
        pub code: String,
    }

    /// Response to a successful `POST /shares/claim` — everything the guest
    /// surface needs to start rendering, without a second round trip.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ClaimResponse {
        /// The owner's Cognito sub: the ctl topic space this grant addresses.
        pub owner_sub: String,
        /// How the owner appears in the guest's "Shared with you" list (their
        /// account email, recorded when the code was minted).
        pub owner_label: String,
        pub setup_id: String,
        #[serde(default)]
        pub setup_name: Option<String>,
    }

    /// One grant as the **owner** sees it (`granted` in [`SharesResponse`]).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Grant {
        /// The contact's Cognito sub — the revoke route's key, and what a
        /// guest's presence card and frames are attributed to.
        pub contact_sub: String,
        /// The contact's account email, recorded when they claimed.
        pub contact_label: String,
        pub setup_id: String,
        #[serde(default)]
        pub setup_name: Option<String>,
        /// The owner's own note from [`InviteRequest::label`], if they set one.
        #[serde(default)]
        pub label: Option<String>,
        /// Epoch millis (server clock).
        pub created_at: i64,
    }

    /// One grant as the **contact** sees it (`received` in [`SharesResponse`]).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ReceivedGrant {
        /// The owner's Cognito sub — the ctl topic space to address.
        pub owner_sub: String,
        pub owner_label: String,
        pub setup_id: String,
        #[serde(default)]
        pub setup_name: Option<String>,
        /// Epoch millis (server clock).
        pub created_at: i64,
    }

    /// An outstanding invite on the owner's list. The code itself is not here
    /// and cannot be: only its hash is stored, so a lost code is withdrawn and
    /// re-minted rather than recovered.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PendingInvite {
        /// Handle for `DELETE /shares/invite/{codeRef}`.
        pub code_ref: String,
        pub setup_id: String,
        #[serde(default)]
        pub setup_name: Option<String>,
        #[serde(default)]
        pub label: Option<String>,
        /// Epoch millis (server clock).
        pub created_at: i64,
        pub expires_at: i64,
    }

    /// Response to `GET /shares` — both directions plus outstanding invites in
    /// one call, because every surface that shows one shows the others.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SharesResponse {
        /// Grants the caller has given away (they are the owner).
        pub granted: Vec<Grant>,
        /// Grants the caller has received (they are the contact).
        pub received: Vec<ReceivedGrant>,
        /// The caller's unclaimed, unexpired invites.
        #[serde(default)]
        pub pending: Vec<PendingInvite>,
    }

    /// Response to any successful `DELETE /shares/…` — revoke, leave, and
    /// withdraw all answer the same shape.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RevokeResponse {
        pub revoked: bool,
    }
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

    /// `{"changed":"shares"}` — published to **both** parties after a grant
    /// changes (claim, revoke, leave, account deletion). Same contract as its
    /// siblings: opaque, any frame means pull. It reaches a party who is not
    /// the writer, which is new for this channel and entirely within the
    /// existing policy — the frame goes to each affected user's own topic.
    pub fn shares_changed_frame() -> String {
        serde_json::json!({ "changed": "shares" }).to_string()
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
    //! - `…/setup/<setupId>/config` — retained compiled setup ([`Config`]) for
    //!   surfaces that hold no synced copy of it: shared-control guests today,
    //!   render nodes later. An empty retained payload clears it.
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

    /// Retained compiled setup ([`Config`]) for surfaces that never sync —
    /// shared-control guests today, render nodes later.
    pub fn config_topic(sub: &str, setup_id: &str) -> String {
        format!("{}/setup/{setup_id}/config", user_prefix(sub))
    }

    /// Retained presence card for one connection (`session` = the random
    /// client-id suffix the connection already mints).
    pub fn presence_topic(sub: &str, session: &str) -> String {
        format!("{}/presence/{session}", user_prefix(sub))
    }

    /// A shared-control guest's presence card in the **owner's** space, so the
    /// owner's desk can show who else is live on it.
    ///
    /// Deliberately keyed by the guest's sub and **not** by session, unlike
    /// [`presence_topic`]. The authorizer runs during the WebSocket handshake,
    /// before any session id exists, so a per-session topic could only be
    /// authorized as a wildcard — and a wildcard over a topic namespace is an
    /// unbounded retained-write primitive: a guest could retain messages on
    /// arbitrarily many topics, the owner would replay every one of them on
    /// each reconnect, and revoking the grant would remove the guest's ability
    /// to clear them, making the mess permanent. An exact topic has no such
    /// hazard and needs no wildcard at all.
    ///
    /// Per-session granularity buys nothing here anyway: a connection has one
    /// Last Will and it stays on the guest's *own* presence topic, so cards in
    /// the owner's space are explicit publish/clear either way. The cost is
    /// that a guest signed in on two devices shows one card, last write wins.
    pub fn guest_presence_topic(owner_sub: &str, contact_sub: &str) -> String {
        format!("{}/presence/{contact_sub}", user_prefix(owner_sub))
    }

    /// One live control write. The two kinds mirror the command layer's two
    /// buffer mutations, so concurrent editors compose instead of clobbering
    /// each other with stale full snapshots: a fader drag touches one slot, a
    /// color-pick overlays the leading slots, and cross-device races resolve
    /// per-slot last-write-wins at the applier.
    ///
    /// `src` is the publishing connection's session id. Every peer subscribes
    /// its whole ctl space, so a publisher receives its own frames back —
    /// appliers drop frames whose `src` matches their own session instead of
    /// re-applying them. Optional on the wire so hand-published frames (CLI
    /// testing) stay valid; absent means "not mine, apply it".
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum Frame {
        /// `{"v":1,"buffer":[…]}` — overlay onto the leading slots, exactly
        /// `LuxBuffer::set` semantics (higher slots untouched).
        Buffer {
            v: u32,
            buffer: Vec<u8>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            src: Option<String>,
        },
        /// `{"v":1,"ch":10,"val":200}` — one slot; `ch` is the 1-based DMX
        /// slot number, matching `set_channel`.
        Channel {
            v: u32,
            ch: u16,
            val: u8,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            src: Option<String>,
        },
    }

    impl Frame {
        pub fn buffer(buffer: Vec<u8>) -> Self {
            Frame::Buffer {
                v: VERSION,
                buffer,
                src: None,
            }
        }

        pub fn channel(ch: u16, val: u8) -> Self {
            Frame::Channel {
                v: VERSION,
                ch,
                val,
                src: None,
            }
        }

        /// Stamp the publishing connection's session id (see the enum docs).
        pub fn with_src(mut self, session: &str) -> Self {
            match &mut self {
                Frame::Buffer { src, .. } | Frame::Channel { src, .. } => {
                    *src = Some(session.to_owned());
                }
            }
            self
        }

        /// The payload's wire version — gate on this before applying.
        pub fn version(&self) -> u32 {
            match self {
                Frame::Buffer { v, .. } | Frame::Channel { v, .. } => *v,
            }
        }

        /// The publishing connection's session id, if stamped.
        pub fn src(&self) -> Option<&str> {
            match self {
                Frame::Buffer { src, .. } | Frame::Channel { src, .. } => src.as_deref(),
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

    /// A setup compiled down to what it takes to *render a surface for it* —
    /// published retained by the owner's applier so a consumer that never syncs
    /// can draw the desk: shared-control guests today, render nodes later.
    ///
    /// This is deliberately not a [`super::SetupRecord`]. That shape is the
    /// authoring model (opaque fixture JSON, revisions, tombstones) and it
    /// belongs to accounts that own it. This one is the *rendering* model:
    /// flat, small, and parseable by an embedded node with no JSON DOM — one
    /// object, two arrays of fixed-shape objects, no nesting beyond that, no
    /// maps, no optional-object soup.
    ///
    /// `role` is a plain string rather than an enum on purpose. A consumer
    /// matches the roles it knows and treats the rest as a plain fader, so
    /// adding a role never strands an older reader — the same
    /// reader-drops-what-it-doesn't-know discipline as [`Frame`]'s `v`.
    ///
    /// `channels` lists only *patched* slots. An empty list is not an error and
    /// not an empty desk: it means the setup has no patch, and a surface should
    /// render the plain universe (matching what the app already does locally).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Config {
        pub v: u32,
        pub setup_id: String,
        /// The setup's display name, as the owner named it.
        pub name: String,
        pub universe: u16,
        pub channels: Vec<ConfigChannel>,
        pub fixtures: Vec<ConfigFixture>,
    }

    /// One patched DMX slot in a [`Config`].
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ConfigChannel {
        /// 1-based DMX slot number, matching [`Frame::Channel`]'s `ch`.
        pub n: u16,
        /// The channel's label from the patch.
        pub name: String,
        /// Semantic role driving the control affordance and colour ("Red",
        /// "Brightness", "Generic", …). See the type docs on why it's a string.
        pub role: String,
    }

    /// One patched fixture in a [`Config`] — enough to group the channels above
    /// under a heading, not the fixture's authoring definition.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ConfigFixture {
        pub name: String,
        /// 1-based address of the fixture's first slot.
        pub address: u16,
        /// How many consecutive slots it occupies.
        pub count: u16,
    }

    impl Config {
        pub fn new(
            setup_id: String,
            name: String,
            universe: u16,
            channels: Vec<ConfigChannel>,
            fixtures: Vec<ConfigFixture>,
        ) -> Self {
            Self {
                v: VERSION,
                setup_id,
                name,
                universe,
                channels,
                fixtures,
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
    fn apple_sign_in_request_shape() {
        // First-authorization shape: email + name present.
        let first = apple::SignInRequest {
            identity_token: "eyJ0.a.b".into(),
            authorization_code: "c0de".into(),
            raw_nonce: "f3a1".into(),
            email: Some("user@example.com".into()),
            full_name: Some("Ada Lovelace".into()),
        };
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            r#"{"identityToken":"eyJ0.a.b","authorizationCode":"c0de","rawNonce":"f3a1","email":"user@example.com","fullName":"Ada Lovelace"}"#
        );

        // Every later authorization: Apple omits them; `null` on the wire.
        let later = apple::SignInRequest {
            identity_token: "eyJ0.a.b".into(),
            authorization_code: "c0de".into(),
            raw_nonce: "f3a1".into(),
            email: None,
            full_name: None,
        };
        assert_eq!(
            serde_json::to_string(&later).unwrap(),
            r#"{"identityToken":"eyJ0.a.b","authorizationCode":"c0de","rawNonce":"f3a1","email":null,"fullName":null}"#
        );

        // A body without the optional fields at all still parses.
        let bare: apple::SignInRequest =
            serde_json::from_str(r#"{"identityToken":"t","authorizationCode":"c","rawNonce":"n"}"#)
                .unwrap();
        assert!(bare.email.is_none() && bare.full_name.is_none());
    }

    #[test]
    fn apple_sign_in_response_shape() {
        let body = apple::SignInResponse {
            id_token: "id".into(),
            access_token: "ac".into(),
            refresh_token: "re".into(),
            expires_in: 3600,
            created: true,
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"idToken":"id","accessToken":"ac","refreshToken":"re","expiresIn":3600,"created":true}"#
        );

        // A reply without `created` (defaulted) still parses.
        let old: apple::SignInResponse = serde_json::from_str(
            r#"{"idToken":"id","accessToken":"ac","refreshToken":"re","expiresIn":3600}"#,
        )
        .unwrap();
        assert!(!old.created);
    }

    #[test]
    fn apple_link_and_revoke_shapes() {
        assert_eq!(
            serde_json::to_string(&apple::LinkResponse { linked: true }).unwrap(),
            r#"{"linked":true}"#
        );
        assert_eq!(
            serde_json::to_string(&apple::RevokeResponse { revoked: true }).unwrap(),
            r#"{"revoked":true}"#
        );
    }

    #[test]
    fn apple_segments() {
        assert_eq!(
            format!("/{}/{}", apple::AUTH_SEGMENT, apple::APPLE_SEGMENT),
            "/auth/apple"
        );
        assert_eq!(apple::LINK_SEGMENT, "link");
        assert_eq!(apple::REVOKE_SEGMENT, "revoke");
    }

    #[test]
    fn device_authorize_shapes() {
        let req = device::AuthorizeRequest {
            device_id: "d-1".into(),
            hostname: "venue-node".into(),
            mac_tail: "2dae".into(),
            version: "1.5.0".into(),
            arch: "aarch64".into(),
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"deviceId":"d-1","hostname":"venue-node","macTail":"2dae","version":"1.5.0","arch":"aarch64"}"#
        );
        let resp = device::AuthorizeResponse {
            device_code: "secret".into(),
            user_code: "LUX-7QK2".into(),
            interval: 5,
            expires_in: 900,
        };
        assert_eq!(
            serde_json::to_string(&resp).unwrap(),
            r#"{"deviceCode":"secret","userCode":"LUX-7QK2","interval":5,"expiresIn":900}"#
        );
    }

    #[test]
    fn device_token_shapes() {
        // Pending: bare status, no nulls on the wire.
        let pending = device::TokenResponse {
            status: "authorization_pending".into(),
            email: None,
            refresh_token: None,
            client_id: None,
            setup_id: None,
            universe: None,
        };
        assert_eq!(
            serde_json::to_string(&pending).unwrap(),
            r#"{"status":"authorization_pending"}"#
        );
        let granted = device::TokenResponse {
            status: "granted".into(),
            email: Some("a@b.c".into()),
            refresh_token: Some("re".into()),
            client_id: Some("dev-client".into()),
            setup_id: Some("s-1".into()),
            universe: Some(1),
        };
        assert_eq!(
            serde_json::to_string(&granted).unwrap(),
            r#"{"status":"granted","email":"a@b.c","refreshToken":"re","clientId":"dev-client","setupId":"s-1","universe":1}"#
        );
    }

    #[test]
    fn device_approve_defaults() {
        // An approve without universe/name (the app's minimal form) parses.
        let req: device::ApproveRequest =
            serde_json::from_str(r#"{"pairRef":"abc","setupId":"s-1"}"#).unwrap();
        assert_eq!(req.universe, None);
        assert_eq!(req.name, None);
    }

    #[test]
    fn device_segments() {
        assert_eq!(
            format!(
                "/{}/{}/{}",
                apple::AUTH_SEGMENT,
                device::DEVICE_SEGMENT,
                device::AUTHORIZE_SEGMENT
            ),
            "/auth/device/authorize"
        );
        assert_eq!(device::TOKEN_SEGMENT, "token");
        assert_eq!(device::PENDING_SEGMENT, "pending");
        assert_eq!(device::APPROVE_SEGMENT, "approve");
        assert_eq!(device::LIST_SEGMENT, "list");
        assert_eq!(device::REVOKE_SEGMENT, "revoke");
    }

    #[test]
    fn device_list_and_revoke_shapes() {
        let record = device::DeviceRecord {
            device_id: "d-1".into(),
            name: "Stage node".into(),
            hostname: "venue-node".into(),
            setup_id: "s-1".into(),
            universe: 1,
            paired_at: 1_700_000_000_000,
        };
        assert_eq!(
            serde_json::to_string(&record).unwrap(),
            r#"{"deviceId":"d-1","name":"Stage node","hostname":"venue-node","setupId":"s-1","universe":1,"pairedAt":1700000000000}"#
        );
        assert_eq!(
            serde_json::to_string(&device::ListResponse {
                devices: vec![record],
            })
            .unwrap(),
            r#"{"devices":[{"deviceId":"d-1","name":"Stage node","hostname":"venue-node","setupId":"s-1","universe":1,"pairedAt":1700000000000}]}"#
        );
        assert_eq!(
            serde_json::to_string(&device::ApproveResponse { approved: true }).unwrap(),
            r#"{"approved":true}"#
        );

        // Revoke: request parses the app's body, response is a bare flag.
        let req: device::RevokeRequest = serde_json::from_str(r#"{"deviceId":"d-1"}"#).unwrap();
        assert_eq!(req.device_id, "d-1");
        assert_eq!(
            serde_json::to_string(&device::RevokeResponse { revoked: true }).unwrap(),
            r#"{"revoked":true}"#
        );
    }

    #[test]
    fn nudge_frame_and_topics() {
        assert_eq!(nudge::setups_changed_frame(), r#"{"changed":"setups"}"#);
        assert_eq!(nudge::settings_changed_frame(), r#"{"changed":"settings"}"#);
        assert_eq!(nudge::shares_changed_frame(), r#"{"changed":"shares"}"#);
        assert_eq!(nudge::user_topic("abc-123"), "lux/sync/user/abc-123");
        assert_eq!(nudge::client_id_prefix("abc-123"), "lux-sync-abc-123-");
    }

    #[test]
    fn shares_invite_shapes() {
        let req = shares::InviteRequest {
            setup_id: "s-1".into(),
            label: Some("Chelsea".into()),
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"setupId":"s-1","label":"Chelsea"}"#
        );

        // The label is optional — an invite minted without one still parses.
        let bare: shares::InviteRequest = serde_json::from_str(r#"{"setupId":"s-1"}"#).unwrap();
        assert!(bare.label.is_none());

        let resp = shares::InviteResponse {
            code: "LUX-4KPT9-XQ2WM".into(),
            code_ref: "9f86d081".into(),
            expires_at: 1719000000000,
        };
        assert_eq!(
            serde_json::to_string(&resp).unwrap(),
            r#"{"code":"LUX-4KPT9-XQ2WM","codeRef":"9f86d081","expiresAt":1719000000000}"#
        );
    }

    #[test]
    fn shares_claim_shapes() {
        assert_eq!(
            serde_json::to_string(&shares::ClaimRequest {
                code: "LUX-4KPT9-XQ2WM".into()
            })
            .unwrap(),
            r#"{"code":"LUX-4KPT9-XQ2WM"}"#
        );

        let resp = shares::ClaimResponse {
            owner_sub: "owner-1".into(),
            owner_label: "owner@example.com".into(),
            setup_id: "s-1".into(),
            setup_name: Some("Living room".into()),
        };
        assert_eq!(
            serde_json::to_string(&resp).unwrap(),
            r#"{"ownerSub":"owner-1","ownerLabel":"owner@example.com","setupId":"s-1","setupName":"Living room"}"#
        );

        // A setup whose name the server couldn't resolve still claims.
        let nameless: shares::ClaimResponse = serde_json::from_str(
            r#"{"ownerSub":"o","ownerLabel":"l","setupId":"s","setupName":null}"#,
        )
        .unwrap();
        assert!(nameless.setup_name.is_none());
    }

    #[test]
    fn shares_list_shape() {
        let body = shares::SharesResponse {
            granted: vec![shares::Grant {
                contact_sub: "contact-1".into(),
                contact_label: "contact@example.com".into(),
                setup_id: "s-1".into(),
                setup_name: Some("Living room".into()),
                label: Some("Chelsea".into()),
                created_at: 1719000000000,
            }],
            received: vec![shares::ReceivedGrant {
                owner_sub: "owner-2".into(),
                owner_label: "owner@example.com".into(),
                setup_id: "s-9".into(),
                setup_name: None,
                created_at: 1719000000001,
            }],
            pending: vec![shares::PendingInvite {
                code_ref: "9f86d081".into(),
                setup_id: "s-1".into(),
                setup_name: Some("Living room".into()),
                label: None,
                created_at: 1719000000002,
                expires_at: 1719172800002,
            }],
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"granted":[{"contactSub":"contact-1","contactLabel":"contact@example.com","setupId":"s-1","setupName":"Living room","label":"Chelsea","createdAt":1719000000000}],"received":[{"ownerSub":"owner-2","ownerLabel":"owner@example.com","setupId":"s-9","setupName":null,"createdAt":1719000000001}],"pending":[{"codeRef":"9f86d081","setupId":"s-1","setupName":"Living room","label":null,"createdAt":1719000000002,"expiresAt":1719172800002}]}"#
        );

        // A reply from a server that predates the pending list still parses —
        // shared control ships across an App Review lag, so an older client
        // and a newer server (and the reverse) must both hold.
        let old: shares::SharesResponse =
            serde_json::from_str(r#"{"granted":[],"received":[]}"#).unwrap();
        assert!(old.pending.is_empty());

        assert_eq!(
            serde_json::to_string(&shares::RevokeResponse { revoked: true }).unwrap(),
            r#"{"revoked":true}"#
        );
    }

    #[test]
    fn shares_segments_and_caps() {
        assert_eq!(
            format!("/{}/{}", shares::SHARES_SEGMENT, shares::INVITE_SEGMENT),
            "/shares/invite"
        );
        assert_eq!(shares::CLAIM_SEGMENT, "claim");
        assert_eq!(
            format!(
                "/{}/{}/{}/{}",
                shares::SHARES_SEGMENT,
                shares::GRANTED_SEGMENT,
                "contact-1",
                "s-1"
            ),
            "/shares/granted/contact-1/s-1"
        );
        assert_eq!(shares::RECEIVED_SEGMENT, "received");

        // The grant cap is a protocol constant, not a local policy: the sync
        // API refuses past it and the authorizer truncates at it, so the two
        // must read the same number from here. It is bounded by the IoT
        // authorizer's 10-policy-document limit, one of which is the contact's
        // own space.
        assert_eq!(shares::MAX_GRANTS_PER_CONTACT, 9);
        assert_eq!(shares::INVITE_TTL_SECS, 172_800);
    }

    #[test]
    fn ctl_config_shape() {
        let config = ctl::Config::new(
            "s-1".into(),
            "Living room".into(),
            1,
            vec![ctl::ConfigChannel {
                n: 1,
                name: "Red".into(),
                role: "Red".into(),
            }],
            vec![ctl::ConfigFixture {
                name: "Par 1".into(),
                address: 1,
                count: 5,
            }],
        );
        assert_eq!(
            serde_json::to_string(&config).unwrap(),
            r#"{"v":1,"setupId":"s-1","name":"Living room","universe":1,"channels":[{"n":1,"name":"Red","role":"Red"}],"fixtures":[{"name":"Par 1","address":1,"count":5}]}"#
        );

        // An unpatched setup publishes empty lists, not a missing key: a fixed
        // parser reads the same field set every time.
        let bare = ctl::Config::new("s-2".into(), "Blank".into(), 3, vec![], vec![]);
        assert_eq!(
            serde_json::to_string(&bare).unwrap(),
            r#"{"v":1,"setupId":"s-2","name":"Blank","universe":3,"channels":[],"fixtures":[]}"#
        );

        // A role this reader has never heard of is a string it can ignore, not
        // a parse failure — the whole reason `role` isn't an enum.
        let future: ctl::Config = serde_json::from_str(
            r#"{"v":1,"setupId":"s","name":"n","universe":1,"channels":[{"n":7,"name":"UV","role":"Ultraviolet"}],"fixtures":[]}"#,
        )
        .unwrap();
        assert_eq!(future.channels[0].role, "Ultraviolet");

        // The version gate is the reader's job, so a newer payload of a known
        // shape must survive deserialization to be dropped deliberately.
        let newer: ctl::Config = serde_json::from_str(
            r#"{"v":9,"setupId":"s","name":"n","universe":1,"channels":[],"fixtures":[]}"#,
        )
        .unwrap();
        assert_eq!(newer.v, 9);
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

        // A guest's card lands in the owner's space, keyed by the guest's sub —
        // one exact topic, so the authorizer needs no wildcard to grant it.
        assert_eq!(
            ctl::guest_presence_topic("owner-1", "contact-2"),
            "lux/ctl/user/owner-1/presence/contact-2"
        );
        assert!(!ctl::guest_presence_topic("owner-1", "contact-2").contains('*'));
        // Distinct guests, and guest vs owner, never collide.
        assert_ne!(
            ctl::guest_presence_topic("owner-1", "contact-2"),
            ctl::guest_presence_topic("owner-1", "contact-3")
        );
        assert_ne!(
            ctl::guest_presence_topic("owner-1", "contact-2"),
            ctl::presence_topic("owner-1", "owner-session")
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

        // `src` (the publisher's session id) rides both kinds and is omitted
        // when absent — the unstamped pins above are also the CLI-publish shape.
        assert_eq!(
            serde_json::to_string(&ctl::Frame::channel(10, 200).with_src("0a1b2c3d")).unwrap(),
            r#"{"v":1,"ch":10,"val":200,"src":"0a1b2c3d"}"#
        );
        assert_eq!(
            serde_json::to_string(&ctl::Frame::buffer(vec![1]).with_src("0a1b2c3d")).unwrap(),
            r#"{"v":1,"buffer":[1],"src":"0a1b2c3d"}"#
        );
        let stamped: ctl::Frame =
            serde_json::from_str(r#"{"v":1,"ch":1,"val":9,"src":"0a1b2c3d"}"#).unwrap();
        assert_eq!(stamped.src(), Some("0a1b2c3d"));
        let unstamped: ctl::Frame = serde_json::from_str(r#"{"v":1,"ch":1,"val":9}"#).unwrap();
        assert_eq!(unstamped.src(), None);

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
