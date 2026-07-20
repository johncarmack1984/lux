//! lux-iot-authorizer — IoT Core custom authorizer for the change-nudge channel.
//!
//! The desktop keeps an open MQTT-over-WebSocket connection to IoT Core so the
//! sync-api can nudge it when another device writes (see `lux_wire::nudge`).
//! IoT invokes this Lambda on every connect with the Cognito ID token the app
//! put in the `x-lux-token` handshake header (the authorizer's
//! `token_key_name`); we verify it exactly as the sync-api does (`lux-auth`)
//! and answer with an IoT policy scoped to the *verified* user's own topics —
//! the nudge subscription plus pub/sub over their remote-control (`ctl`)
//! space — and client-id prefix: the same token-derived tenant isolation as
//! the DynamoDB partition key. A bad token gets `isAuthenticated: false`,
//! never a policy.
//!
//! The authorizer is registered with signing disabled: the desktop is a public
//! client and can't hold a signing key, so the JWT check here is the gate —
//! the same posture as the sync-api's public Function URL with in-handler JWT.
//!
//! **Shared control** (docs/shared-control.md) is the one thing that widens a
//! connection past its own owner's space. After the token verifies, we read the
//! caller's `SHARED#<sub>` partition — the grants *other* accounts have given
//! them — and append one narrow policy document per grant. A grant never
//! confers the owner's own rights: a guest may publish live frames and its own
//! presence card, and read the applier's state and config. It cannot retain
//! anything but its own presence, cannot publish state or config (the owner's
//! applier stays the sole authority on both), and cannot touch a setup it was
//! not granted.
//!
//! Two properties are worth stating because the rest of the file depends on
//! them. First, policy construction is pure: [`allow`] takes the grants it was
//! handed and does no I/O, so every statement it can emit is unit-testable, and
//! the DynamoDB read that finds those grants is separate and fallible.
//! Second, revocation is bounded but not instant — a policy lives for the
//! connection's refresh window (`refreshAfterInSeconds`, one hour), so a
//! revoked guest keeps the access they already hold until their next re-auth.
//! That window is the documented cost of connect-time authorization.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

/// AWS IoT accepts at most this many policy documents from a custom authorizer,
/// each at most [`MAX_POLICY_DOCUMENT_CHARS`]. The caller's own space takes the
/// first, so the rest are the grant budget — which is where
/// [`lux_wire::shares::MAX_GRANTS_PER_CONTACT`] comes from.
const MAX_POLICY_DOCUMENTS: usize = 10;
const MAX_POLICY_DOCUMENT_CHARS: usize = 2048;

/// Is this identifier safe to splice into an IAM resource ARN?
///
/// Subs and setup ids are UUIDs in practice, but "in practice" is not a check.
/// IAM wildcards match `/`, so a single `*` reaching an ARN turns one grant
/// into a grant over everything under that path segment — the difference
/// between "one setup" and "every setup this owner will ever have".
fn is_arn_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// One live grant, reduced to what a policy needs: whose space, which setup.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GrantScope {
    owner_sub: String,
    setup_id: String,
}

/// Where grants are read from. `None` when `DYNAMODB_TABLE` is unset, which
/// simply means no connection is ever widened — the pre-shared-control
/// behaviour, and a safe state to deploy into.
struct GrantStore {
    ddb: aws_sdk_dynamodb::Client,
    table: String,
}

/// The slice of IoT's custom-authorizer request we read. Everything is
/// optional-with-default: IoT varies the shape by protocol, and a missing
/// field should read as "no token found", not a deserialization failure.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthEvent {
    /// Set by IoT when the client passed the token under `token_key_name`.
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    protocol_data: Option<ProtocolData>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolData {
    #[serde(default)]
    http: Option<HttpData>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpData {
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    query_string: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    // reqwest uses rustls with no baked provider; install ring as the process
    // default before the JWKS fetch below performs any TLS.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let pool_id = env("COGNITO_USER_POOL_ID")?;
    let client_id = env("COGNITO_APP_CLIENT_ID")?;
    let region = env("COGNITO_REGION")?;

    let verifier = lux_auth::Verifier::new(&region, &pool_id, &client_id)
        .await
        .expect("failed to fetch Cognito JWKS");
    let verifier = &verifier;

    // Shared-control grants. Absent table => nobody's connection is widened.
    let store = match std::env::var("DYNAMODB_TABLE")
        .ok()
        .filter(|t| !t.is_empty())
    {
        Some(table) => {
            let conf = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .http_client(aws_http_client())
                .load()
                .await;
            Some(GrantStore {
                ddb: aws_sdk_dynamodb::Client::new(&conf),
                table,
            })
        }
        None => {
            tracing::info!("DYNAMODB_TABLE unset; shared-control grants disabled");
            None
        }
    };
    let store = store.as_ref();

    run(service_fn(move |event: LambdaEvent<Value>| async move {
        Ok::<Value, Error>(authorize(verifier, store, event).await)
    }))
    .await
}

fn env(key: &str) -> Result<String, Error> {
    std::env::var(key).map_err(|_| format!("missing required env var {key}").into())
}

async fn authorize(
    verifier: &lux_auth::Verifier,
    store: Option<&GrantStore>,
    event: LambdaEvent<Value>,
) -> Value {
    // Region + account for the policy ARNs, from our own function ARN
    // (arn:aws:lambda:<region>:<account>:function:<name>).
    let arn: Vec<&str> = event.context.invoked_function_arn.split(':').collect();
    let (Some(region), Some(account)) = (arn.get(3), arn.get(4)) else {
        tracing::error!("malformed invoked_function_arn; denying");
        return deny();
    };

    let parsed: AuthEvent = match serde_json::from_value(event.payload) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("unreadable authorizer event ({e}); denying");
            return deny();
        }
    };

    let Some(token) = extract_token(&parsed) else {
        tracing::info!("no token on connect; denying");
        return deny();
    };

    let sub = match verifier.verify(&token) {
        Ok(claims) => claims.sub,
        Err(e) => {
            tracing::info!("token rejected ({e}); denying");
            return deny();
        }
    };

    // A grant read that fails grants nothing. It deliberately does not deny the
    // connection outright: this user's own sync and control are not a
    // shared-control feature, and a DynamoDB blip must not sign everyone out.
    // Closed with respect to *shares* — no lookup, no widening, ever — while
    // the caller's own space is unaffected.
    let grants = match store {
        Some(store) => live_grants(store, &sub).await.unwrap_or_else(|e| {
            tracing::error!("grant lookup failed for {sub} ({e}); authorizing own space only");
            Vec::new()
        }),
        None => Vec::new(),
    };

    allow(region, account, &sub, &grants)
}

/// The grants this user holds *as a contact* — one query on their own
/// `SHARED#<sub>` partition, which exists in this shape precisely so that the
/// connect path is a single key lookup and never a scan or a filter over
/// someone else's data.
async fn live_grants(store: &GrantStore, sub: &str) -> Result<Vec<GrantScope>, String> {
    let out = store
        .ddb
        .query()
        .table_name(&store.table)
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S(format!("SHARED#{sub}")))
        .send()
        .await
        .map_err(|e| format!("query failed: {e}"))?;

    let s = |item: &HashMap<String, AttributeValue>, key: &str| item.get(key)?.as_s().ok().cloned();
    Ok(out
        .items()
        .iter()
        .filter_map(|item| {
            Some(GrantScope {
                owner_sub: s(item, "ownerSub")?,
                setup_id: s(item, "setupId")?,
            })
        })
        .collect())
}

/// The allow response for a verified user: connect under their own client-id
/// prefix, the nudge subscription, and full pub/sub over their own
/// remote-control space (`lux_wire::ctl`) — the ctl channel rides the same
/// connection, and the topic scheme scopes every grant to the verified sub.
/// (In the ctl resources, `*` is an IAM string wildcard, so the Subscribe
/// grant matches the client's `…/#` filter; Subscribe takes `topicfilter`
/// ARNs, Publish/Receive take `topic` ARNs. The Publish grant also covers the
/// connection's Last Will on the presence topic.)
fn allow(region: &str, account: &str, sub: &str, grants: &[GrantScope]) -> Value {
    let prefix = format!("arn:aws:iot:{region}:{account}");
    let nudge_topic = lux_wire::nudge::user_topic(sub);
    let client_prefix = lux_wire::nudge::client_id_prefix(sub);
    let ctl_prefix = lux_wire::ctl::user_prefix(sub);

    // One document per grant, after the caller's own. Packing two grants into a
    // document would risk the 2048-character ceiling (a grant runs ~1 KB), and
    // an oversized document is rejected wholesale — so the cheap, checkable
    // arrangement is one each. `documents` therefore holds at most
    // MAX_POLICY_DOCUMENTS entries and the claim route refuses past the same
    // number, which is what keeps this truncation unreachable in practice.
    let mut documents = vec![json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": "iot:Connect",
                "Resource": format!("{prefix}:client/{client_prefix}*"),
            },
            {
                "Effect": "Allow",
                "Action": "iot:Subscribe",
                "Resource": [
                    format!("{prefix}:topicfilter/{nudge_topic}"),
                    format!("{prefix}:topicfilter/{ctl_prefix}/*"),
                ],
            },
            {
                "Effect": "Allow",
                "Action": "iot:Receive",
                "Resource": [
                    format!("{prefix}:topic/{nudge_topic}"),
                    format!("{prefix}:topic/{ctl_prefix}/*"),
                ],
            },
            {
                "Effect": "Allow",
                // RetainPublish too: presence cards, state echoes, and the
                // connection's Last Will are all retained — and IoT refuses
                // the CONNECT itself when the retained will isn't covered.
                "Action": ["iot:Publish", "iot:RetainPublish"],
                "Resource": format!("{prefix}:topic/{ctl_prefix}/*"),
            },
        ],
    })];

    for grant in grants {
        if documents.len() >= MAX_POLICY_DOCUMENTS {
            tracing::error!(
                "{sub} holds more than {} grants; the rest are unauthorized until some are \
                 revoked (the claim route should have refused past {})",
                MAX_POLICY_DOCUMENTS - 1,
                lux_wire::shares::MAX_GRANTS_PER_CONTACT
            );
            break;
        }
        // Nothing unvalidated is ever spliced into an IAM resource. An id
        // carrying `*` would silently widen the grant across the owner's whole
        // setup space, since IAM wildcards match `/` — the ARN builder is the
        // last place that can still refuse, so it does, whatever the write
        // path allowed.
        if !is_arn_safe(&grant.owner_sub) || !is_arn_safe(&grant.setup_id) {
            tracing::error!(
                "grant on {}/{} has an id unsafe for an ARN; skipping it",
                grant.owner_sub,
                grant.setup_id
            );
            continue;
        }
        let document = grant_policy(&prefix, sub, grant);
        // A document over the ceiling is rejected by IoT as a whole, which
        // would take the caller's own access down with it. Dropping the one
        // grant keeps the connection working and says so loudly.
        let size = document.to_string().len();
        if size > MAX_POLICY_DOCUMENT_CHARS {
            tracing::error!(
                "grant on {}/{} builds a {size}-character policy document, over the {} ceiling; \
                 skipping it",
                grant.owner_sub,
                grant.setup_id,
                MAX_POLICY_DOCUMENT_CHARS
            );
            continue;
        }
        documents.push(document);
    }

    json!({
        "isAuthenticated": true,
        // principalId must be alphanumeric; Cognito subs are UUIDs with hyphens.
        "principalId": sub.chars().filter(char::is_ascii_alphanumeric).collect::<String>(),
        // Hourly forced re-auth: matches the ID token's validity, and the
        // client's reconnect brings a fresh token + an on-connect pull. It is
        // also the upper bound on how long a revoked grant keeps working.
        "disconnectAfterInSeconds": 3600,
        "refreshAfterInSeconds": 3600,
        "policyDocuments": documents,
    })
}

/// One grant's policy document: what a guest may do in an owner's space, and
/// nothing else.
///
/// The asymmetry is the design. A guest **publishes** live frames (never
/// retained — the retain grant below is only for their own presence card) and
/// **reads** the applier's `state` and `config`. It cannot publish `state` or
/// `config`: the owner's applier is the sole authority on what the fixtures are
/// doing and what the setup looks like, and a guest that could retain either
/// would be able to lie to every other surface, including after it left.
fn grant_policy(prefix: &str, contact_sub: &str, grant: &GrantScope) -> Value {
    let frame = lux_wire::ctl::frame_topic(&grant.owner_sub, &grant.setup_id);
    let state = lux_wire::ctl::state_topic(&grant.owner_sub, &grant.setup_id);
    let config = lux_wire::ctl::config_topic(&grant.owner_sub, &grant.setup_id);
    let presence = lux_wire::ctl::guest_presence_topic(&grant.owner_sub, contact_sub);

    json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": "iot:Publish",
                "Resource": [
                    format!("{prefix}:topic/{frame}"),
                    format!("{prefix}:topic/{presence}"),
                ],
            },
            {
                "Effect": "Allow",
                // Only the guest's own presence card may be retained, and the
                // resource is wildcarded to their sub — never the owner's card
                // or another guest's.
                "Action": "iot:RetainPublish",
                "Resource": format!("{prefix}:topic/{presence}"),
            },
            {
                "Effect": "Allow",
                "Action": "iot:Subscribe",
                "Resource": [
                    format!("{prefix}:topicfilter/{state}"),
                    format!("{prefix}:topicfilter/{config}"),
                ],
            },
            {
                "Effect": "Allow",
                "Action": "iot:Receive",
                "Resource": [
                    format!("{prefix}:topic/{state}"),
                    format!("{prefix}:topic/{config}"),
                ],
            },
        ],
    })
}

/// The token, wherever IoT surfaced it: the extracted top-level field when the
/// client used `token_key_name`, else the raw handshake header or query param.
fn extract_token(event: &AuthEvent) -> Option<String> {
    if let Some(token) = event.token.clone().filter(|t| !t.is_empty()) {
        return Some(token);
    }
    let http = event.protocol_data.as_ref()?.http.as_ref()?;
    if let Some((_, v)) = http
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(lux_wire::nudge::TOKEN_KEY))
    {
        if !v.is_empty() {
            return Some(v.clone());
        }
    }
    // JWTs are URL-safe (base64url + dots), so a plain key=value scan suffices.
    http.query_string
        .as_deref()?
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| k.eq_ignore_ascii_case(lux_wire::nudge::TOKEN_KEY))
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty())
}

fn deny() -> Value {
    json!({
        "isAuthenticated": false,
        "principalId": "denied",
        "disconnectAfterInSeconds": 300,
        "refreshAfterInSeconds": 300,
        "policyDocuments": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(payload: Value) -> AuthEvent {
        serde_json::from_value(payload).unwrap()
    }

    #[test]
    fn token_from_top_level_field() {
        let e = event(json!({ "token": "abc" }));
        assert_eq!(extract_token(&e).as_deref(), Some("abc"));
    }

    #[test]
    fn token_from_handshake_header_any_case() {
        let e = event(json!({
            "protocolData": { "http": { "headers": { "X-Lux-Token": "abc" } } }
        }));
        assert_eq!(extract_token(&e).as_deref(), Some("abc"));
    }

    #[test]
    fn token_from_query_string() {
        let e = event(json!({
            "protocolData": { "http": {
                "queryString": "?x-amz-customauthorizer-name=lux-sync-auth&x-lux-token=abc"
            } }
        }));
        assert_eq!(extract_token(&e).as_deref(), Some("abc"));
    }

    #[test]
    fn missing_token_is_none() {
        let e = event(json!({ "protocolData": { "http": { "headers": {} } } }));
        assert_eq!(extract_token(&e), None);
        assert_eq!(extract_token(&event(json!({}))), None);
    }

    fn grant(owner: &str, setup: &str) -> GrantScope {
        GrantScope {
            owner_sub: owner.to_owned(),
            setup_id: setup.to_owned(),
        }
    }

    /// A realistically-sized identity pair: Cognito subs and setup ids are both
    /// UUIDs, and the policy budget is measured in characters.
    const UUID_A: &str = "7f0175a6-3b64-4a2a-9e1c-000000000001";
    const UUID_B: &str = "7f0175a6-3b64-4a2a-9e1c-000000000002";

    #[test]
    fn allow_policy_grants_nudge_and_ctl_scoped_to_the_sub() {
        let response = allow("us-west-1", "735853783919", "abc-123", &[]);
        assert_eq!(response["isAuthenticated"], true);
        assert_eq!(response["principalId"], "abc123"); // non-alphanumerics dropped

        // No grants, no extra documents: an ordinary user's policy is exactly
        // what it was before shared control existed.
        assert_eq!(response["policyDocuments"][1], Value::Null);

        let statements = &response["policyDocuments"][0]["Statement"];
        let arn = |suffix: &str| format!("arn:aws:iot:us-west-1:735853783919:{suffix}");

        assert_eq!(statements[0]["Action"], "iot:Connect");
        assert_eq!(
            statements[0]["Resource"],
            arn("client/lux-sync-abc-123-*").as_str()
        );

        assert_eq!(statements[1]["Action"], "iot:Subscribe");
        assert_eq!(
            statements[1]["Resource"],
            json!([
                arn("topicfilter/lux/sync/user/abc-123"),
                arn("topicfilter/lux/ctl/user/abc-123/*"),
            ])
        );

        assert_eq!(statements[2]["Action"], "iot:Receive");
        assert_eq!(
            statements[2]["Resource"],
            json!([
                arn("topic/lux/sync/user/abc-123"),
                arn("topic/lux/ctl/user/abc-123/*"),
            ])
        );

        // The one genuinely new capability: publish, ctl space only — nothing
        // grants publish on the nudge topic or anyone else's prefix. Retained
        // publish rides along for the presence card, state echo, and the will.
        assert_eq!(
            statements[3]["Action"],
            json!(["iot:Publish", "iot:RetainPublish"])
        );
        assert_eq!(
            statements[3]["Resource"],
            arn("topic/lux/ctl/user/abc-123/*").as_str()
        );
        // json indexing past the end yields Null — asserts there is no fifth
        // statement without unwrapping.
        assert_eq!(statements[4], Value::Null);
    }

    #[test]
    fn a_grant_adds_one_document_and_nothing_to_the_callers_own() {
        let base = allow("us-west-1", "735853783919", "contact-1", &[]);
        let shared = allow(
            "us-west-1",
            "735853783919",
            "contact-1",
            &[grant("owner-9", "s-1")],
        );

        // The caller's own document is untouched by holding a grant.
        assert_eq!(base["policyDocuments"][0], shared["policyDocuments"][0]);
        assert_eq!(shared["policyDocuments"].as_array().map(Vec::len), Some(2));

        let arn = |suffix: &str| format!("arn:aws:iot:us-west-1:735853783919:{suffix}");
        let statements = &shared["policyDocuments"][1]["Statement"];

        // Publish: live frames and the guest's own presence card. Note what is
        // absent — no state, no config, no other setup.
        assert_eq!(statements[0]["Action"], "iot:Publish");
        assert_eq!(
            statements[0]["Resource"],
            json!([
                arn("topic/lux/ctl/user/owner-9/setup/s-1/frame"),
                arn("topic/lux/ctl/user/owner-9/presence/contact-1"),
            ])
        );

        // Retain: presence only. A guest may not leave a retained frame.
        assert_eq!(statements[1]["Action"], "iot:RetainPublish");
        assert_eq!(
            statements[1]["Resource"],
            arn("topic/lux/ctl/user/owner-9/presence/contact-1").as_str()
        );

        assert_eq!(statements[2]["Action"], "iot:Subscribe");
        assert_eq!(
            statements[2]["Resource"],
            json!([
                arn("topicfilter/lux/ctl/user/owner-9/setup/s-1/state"),
                arn("topicfilter/lux/ctl/user/owner-9/setup/s-1/config"),
            ])
        );

        assert_eq!(statements[3]["Action"], "iot:Receive");
        assert_eq!(
            statements[3]["Resource"],
            json!([
                arn("topic/lux/ctl/user/owner-9/setup/s-1/state"),
                arn("topic/lux/ctl/user/owner-9/setup/s-1/config"),
            ])
        );
        assert_eq!(statements[4], Value::Null);
    }

    #[test]
    fn a_grant_confers_nothing_over_the_owners_wider_space() {
        let shared = allow(
            "us-west-1",
            "735853783919",
            "contact-1",
            &[grant("owner-9", "s-1")],
        );
        let policy = shared["policyDocuments"].to_string();

        // Every resource naming the owner is pinned to the granted setup or to
        // this guest's own presence prefix. A wildcard over the owner's ctl
        // space, their nudge topic, their client ids, or a second setup would
        // all show up here.
        assert!(!policy.contains("lux/ctl/user/owner-9/*"));
        assert!(!policy.contains("lux/ctl/user/owner-9/#"));
        assert!(!policy.contains("lux/sync/user/owner-9"));
        assert!(!policy.contains("client/lux-sync-owner-9"));
        assert!(!policy.contains("presence/*"));
        // The guest's presence resource is an exact topic: no wildcard means
        // no unbounded retained-write namespace, and nothing to leave behind
        // that a revoked guest could no longer clear.
        assert!(!policy.contains("presence/contact-1-"));
        assert!(!policy.contains("setup/s-2"));
        // …and the guest cannot write the two topics the owner's applier owns.
        for forbidden in ["setup/s-1/state", "setup/s-1/config"] {
            let publishable = shared["policyDocuments"][1]["Statement"][0]["Resource"].to_string()
                + &shared["policyDocuments"][1]["Statement"][1]["Resource"].to_string();
            assert!(
                !publishable.contains(forbidden),
                "{forbidden} is publishable"
            );
        }
    }

    #[test]
    fn ids_unsafe_for_an_arn_never_reach_one() {
        // `PUT /setups/*` is a legal request, so a setup really can be named
        // `*`. Spliced into an ARN it would widen the grant across every setup
        // the owner has, because IAM wildcards match `/`. The same goes for an
        // id carrying a path separator or an over-long one.
        for bad in ["*", "s-1/../s-2", "a/b", "", &"x".repeat(65)] {
            let response = allow(
                "us-west-1",
                "735853783919",
                "contact-1",
                &[grant(UUID_A, bad)],
            );
            assert_eq!(
                response["policyDocuments"].as_array().map(Vec::len),
                Some(1),
                "an id of {bad:?} produced a policy document"
            );
        }
        // A hostile owner sub is refused the same way.
        let response = allow(
            "us-west-1",
            "735853783919",
            "contact-1",
            &[grant("*", "s-1")],
        );
        assert_eq!(
            response["policyDocuments"].as_array().map(Vec::len),
            Some(1)
        );

        // …and one bad grant does not cost the caller their good ones.
        let response = allow(
            "us-west-1",
            "735853783919",
            "contact-1",
            &[grant(UUID_A, "*"), grant(UUID_A, UUID_B)],
        );
        assert_eq!(
            response["policyDocuments"].as_array().map(Vec::len),
            Some(2)
        );
        assert!(response["policyDocuments"][1].to_string().contains(UUID_B));
    }

    #[test]
    fn grants_stay_inside_the_iot_policy_budget() {
        // The real shapes: UUID subs, UUID setup ids, a full grant load. If a
        // topic ever grows and this trips, the cap in lux-wire is what moves —
        // silently emitting documents IoT rejects is the failure this prevents.
        let grants: Vec<GrantScope> = (0..lux_wire::shares::MAX_GRANTS_PER_CONTACT)
            .map(|i| grant(UUID_A, &format!("{UUID_B}{i}")))
            .collect();
        let response = allow("us-west-1", "735853783919", UUID_A, &grants);
        let documents = response["policyDocuments"]
            .as_array()
            .expect("policyDocuments is an array");

        // Every grant made it in, and the whole set fits the 10-document limit.
        assert_eq!(
            documents.len(),
            lux_wire::shares::MAX_GRANTS_PER_CONTACT + 1
        );
        assert!(documents.len() <= MAX_POLICY_DOCUMENTS);
        for document in documents {
            let size = document.to_string().len();
            assert!(
                size <= MAX_POLICY_DOCUMENT_CHARS,
                "document is {size} chars"
            );
        }
    }

    #[test]
    fn grants_past_the_document_budget_are_dropped_not_smuggled() {
        // The claim route refuses past the cap, so reaching this means the
        // table disagrees with the API — the connection must still work, minus
        // the grants that don't fit.
        let grants: Vec<GrantScope> = (0..MAX_POLICY_DOCUMENTS + 5)
            .map(|i| grant(UUID_A, &format!("setup-{i}")))
            .collect();
        let response = allow("us-west-1", "735853783919", UUID_B, &grants);
        let documents = response["policyDocuments"]
            .as_array()
            .expect("policyDocuments is an array");

        assert_eq!(documents.len(), MAX_POLICY_DOCUMENTS);
        // Truncation never costs the caller their own space, which is first.
        assert_eq!(response["isAuthenticated"], true);
        assert!(documents[0].to_string().contains("iot:Connect"));
    }
}

/// The AWS SDK's HTTP client, built explicitly rather than taken from
/// `aws-config`'s default.
///
/// The bundled default pulls hyper-rustls 0.24 → rustls 0.21 →
/// rustls-webpki 0.101 (four open advisories) for a server-side TLS acceptor
/// type nothing here uses. Building the client ourselves on rustls 0.23 keeps
/// exactly one TLS stack in the binary, and it is the same construction the
/// desktop and node already use.
fn aws_http_client() -> aws_smithy_runtime_api::client::http::SharedHttpClient {
    aws_smithy_http_client::Builder::new()
        .tls_provider(aws_smithy_http_client::tls::Provider::Rustls(
            aws_smithy_http_client::tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https()
}
