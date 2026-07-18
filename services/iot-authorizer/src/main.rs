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

use std::collections::HashMap;

use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

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

    run(service_fn(move |event: LambdaEvent<Value>| async move {
        Ok::<Value, Error>(authorize(verifier, event))
    }))
    .await
}

fn env(key: &str) -> Result<String, Error> {
    std::env::var(key).map_err(|_| format!("missing required env var {key}").into())
}

fn authorize(verifier: &lux_auth::Verifier, event: LambdaEvent<Value>) -> Value {
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

    allow(region, account, &sub)
}

/// The allow response for a verified user: connect under their own client-id
/// prefix, the nudge subscription, and full pub/sub over their own
/// remote-control space (`lux_wire::ctl`) — the ctl channel rides the same
/// connection, and the topic scheme scopes every grant to the verified sub.
/// (In the ctl resources, `*` is an IAM string wildcard, so the Subscribe
/// grant matches the client's `…/#` filter; Subscribe takes `topicfilter`
/// ARNs, Publish/Receive take `topic` ARNs. The Publish grant also covers the
/// connection's Last Will on the presence topic.)
fn allow(region: &str, account: &str, sub: &str) -> Value {
    let prefix = format!("arn:aws:iot:{region}:{account}");
    let nudge_topic = lux_wire::nudge::user_topic(sub);
    let client_prefix = lux_wire::nudge::client_id_prefix(sub);
    let ctl_prefix = lux_wire::ctl::user_prefix(sub);

    json!({
        "isAuthenticated": true,
        // principalId must be alphanumeric; Cognito subs are UUIDs with hyphens.
        "principalId": sub.chars().filter(char::is_ascii_alphanumeric).collect::<String>(),
        // Hourly forced re-auth: matches the ID token's validity, and the
        // client's reconnect brings a fresh token + an on-connect pull.
        "disconnectAfterInSeconds": 3600,
        "refreshAfterInSeconds": 3600,
        "policyDocuments": [{
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
                    "Action": "iot:Publish",
                    "Resource": format!("{prefix}:topic/{ctl_prefix}/*"),
                },
            ],
        }],
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

    #[test]
    fn allow_policy_grants_nudge_and_ctl_scoped_to_the_sub() {
        let response = allow("us-west-1", "735853783919", "abc-123");
        assert_eq!(response["isAuthenticated"], true);
        assert_eq!(response["principalId"], "abc123"); // non-alphanumerics dropped

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
        // grants publish on the nudge topic or anyone else's prefix.
        assert_eq!(statements[3]["Action"], "iot:Publish");
        assert_eq!(
            statements[3]["Resource"],
            arn("topic/lux/ctl/user/abc-123/*").as_str()
        );
        // json indexing past the end yields Null — asserts there is no fifth
        // statement without unwrapping.
        assert_eq!(statements[4], Value::Null);
    }
}
