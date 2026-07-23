//! lux-apple-auth — the Sign in with Apple bridge, on AWS Lambda.
//!
//! The desktop runs the native `ASAuthorizationController` sheet and posts the
//! resulting Apple identity token to this Function URL (`lux_wire::apple`).
//! The handler verifies the token against Apple's JWKS (audience = the app's
//! bundle id, nonce-bound), maps Apple's stable `sub` to a Cognito user via
//! items in the `lux-sync` table (`APPLE#…`/`APPLELINK#…` partitions — never
//! the pool schema, never the sync partitions), and mints ordinary user-pool
//! JWTs through the pool's CUSTOM_AUTH flow. The same binary also serves that
//! flow's three Cognito trigger events (`triggers`), so the challenge's
//! verifier is this exact code: even a client calling Cognito directly can
//! only ever custom-auth into the account its Apple token is linked to.
//!
//! Invoke surfaces, branched on payload shape (`triggerSource` is only ever
//! present on Cognito trigger events):
//! - Function URL: `POST /auth/apple` (sign in / first-use create-or-link),
//!   `POST /auth/apple/link` (bearer-authed explicit link — the Hide My Email
//!   path), `POST /auth/apple/revoke` (bearer-authed; account deletion's
//!   required Apple token revocation).
//! - Cognito triggers: DefineAuthChallenge / CreateAuthChallenge /
//!   VerifyAuthChallengeResponse.
//!
//! The Apple-side private key (`lux/siwa-key`, Secrets Manager) is loaded
//! lazily and cached: it is only needed for the code-exchange and revoke calls
//! to Apple, so token verification and the trigger path keep working — and P1
//! ships dark — before the secret is seeded.

mod apple;
mod cognito;
mod device;
mod http;
mod store;
mod triggers;
mod web;

use std::sync::Arc;

use aws_config::BehaviorVersion;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde_json::Value;

pub(crate) struct Ctx {
    pub cognito: aws_sdk_cognitoidentityprovider::Client,
    pub ddb: aws_sdk_dynamodb::Client,
    pub secrets: aws_sdk_secretsmanager::Client,
    pub verifier: lux_auth::Verifier,
    pub apple: apple::AppleAuth,
    /// The Apple-side signing key, loaded from Secrets Manager on first use.
    pub siwa_key: tokio::sync::OnceCell<apple::SiwaKey>,
    pub pool_id: String,
    pub client_id: String,
    /// The device app client (`lux-node-device`) the pairing grant mints on.
    /// Absent until the infra ships it — the `/auth/device/*` routes and the
    /// device trigger path stay disabled (fail closed) without it.
    pub device_client_id: Option<String>,
    pub table: String,
    pub siwa_secret_id: String,
    /// The web-flow redirect URI registered on the Services ID (the Apple
    /// `/web/callback`). Absent — like the Services ID on `apple` — leaves the
    /// web routes dark (`/web/start` 404s), so the `.dmg` button stays off.
    pub apple_web_callback_url: Option<String>,
}

impl Ctx {
    /// The Apple-side private key, fetched once per warm container. An unseeded
    /// or malformed secret is a runtime error on the routes that talk to Apple,
    /// never a startup crash — verification-only paths must keep working.
    pub async fn siwa_key(&self) -> Result<&apple::SiwaKey, String> {
        self.siwa_key
            .get_or_try_init(|| async {
                let out = self
                    .secrets
                    .get_secret_value()
                    .secret_id(&self.siwa_secret_id)
                    .send()
                    .await
                    .map_err(|e| format!("siwa key secret read failed: {e}"))?;
                let raw = out
                    .secret_string()
                    .ok_or("siwa key secret has no string value")?;
                serde_json::from_str::<apple::SiwaKey>(raw)
                    .map_err(|e| format!("siwa key secret is malformed: {e}"))
            })
            .await
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    // reqwest uses rustls with no baked provider; install ring as the process
    // default before any TLS (Cognito JWKS below, Apple JWKS on first verify).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let pool_id = env("COGNITO_USER_POOL_ID")?;
    let client_id = env("COGNITO_APP_CLIENT_ID")?;
    let device_client_id = std::env::var("COGNITO_DEVICE_CLIENT_ID").ok();
    let region = env("COGNITO_REGION")?;
    let table = env("DYNAMODB_TABLE")?;
    let bundle_id = env("APPLE_BUNDLE_ID")?;
    let siwa_secret_id = env("SIWA_SECRET_ID")?;
    // Web (browser) Sign in with Apple — the `.dmg`/dev fallback. Both must be
    // present for the web routes to light up; either absent keeps them dark.
    let services_id = std::env::var("APPLE_SERVICES_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let apple_web_callback_url = std::env::var("APPLE_WEB_CALLBACK_URL")
        .ok()
        .filter(|s| !s.is_empty());

    let verifier = lux_auth::Verifier::new(&region, &pool_id, &client_id)
        .await
        .expect("failed to fetch Cognito JWKS");

    let conf = aws_config::defaults(BehaviorVersion::latest())
        .http_client(aws_http_client())
        .load()
        .await;

    let ctx = Arc::new(Ctx {
        cognito: aws_sdk_cognitoidentityprovider::Client::new(&conf),
        ddb: aws_sdk_dynamodb::Client::new(&conf),
        secrets: aws_sdk_secretsmanager::Client::new(&conf),
        verifier,
        apple: apple::AppleAuth::new(bundle_id, services_id),
        siwa_key: tokio::sync::OnceCell::new(),
        pool_id,
        client_id,
        device_client_id,
        table,
        siwa_secret_id,
        apple_web_callback_url,
    });

    run(service_fn(move |event: LambdaEvent<Value>| {
        let ctx = ctx.clone();
        async move { handle(ctx, event.payload).await }
    }))
    .await
}

async fn handle(ctx: Arc<Ctx>, payload: Value) -> Result<Value, Error> {
    // Cognito trigger events are the only payloads carrying `triggerSource`;
    // everything else is a Function URL request.
    if payload.get("triggerSource").is_some() {
        triggers::handle(&ctx, payload).await
    } else {
        http::handle(&ctx, payload).await
    }
}

fn env(key: &str) -> Result<String, Error> {
    std::env::var(key).map_err(|_| format!("missing required env var {key}").into())
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
