//! lux-plan-bridge — the Planning Center bridge, on AWS Lambda.
//!
//! The service plan becomes the cue list. A church's administrator authorizes
//! lux against their Planning Center organization once, in a browser; this
//! service keeps custody of the resulting refresh token and is the only thing
//! that ever holds the client secret. Every plan read the app makes comes
//! through here.
//!
//! Why server-side at all, when the desktop could talk to Planning Center
//! directly: one OAuth application serves every church (Planning Center's own
//! guidance), so there is exactly one client secret — and a secret shipped
//! inside an installed app is not a secret. Custody here also means the token
//! survives a reinstall, a new laptop, and the volunteer who set it up leaving.
//!
//! Invoke surface — a Function URL, fronted by the `auth.lux.johncarmack.com`
//! CloudFront distribution that already serves the web Sign in with Apple
//! routes (`infra/apple-auth-web.tf`), because Planning Center registers one
//! exact redirect URI and a raw `*.lambda-url.on.aws` host is not it:
//!
//! - `POST /pco/connect`       — bearer-authed: mint the authorize URL
//! - `GET  /pco/callback`      — Planning Center's redirect; `state`-authed
//! - `GET  /pco/status`        — bearer-authed: is this account connected?
//! - `GET  /pco/service-types` — bearer-authed: what could a setup follow?
//! - `POST /pco/plan`          — bearer-authed: this Sunday, resolved
//! - `POST /pco/disconnect`    — bearer-authed: forget the tokens
//!
//! **Read-only against Planning Center**, structurally: `lux-pco`'s client
//! builds `GET` requests and nothing else, and the `services` scope has no
//! read-only variant to lean on. lux never advances someone else's service.
//!
//! Credentials come from Secrets Manager (`/lux/bridge/prod/pco-oauth`) and are
//! loaded lazily and cached, so the stack applies and the function serves 503s
//! on the connect routes before the secret is seeded — the same fail-soft
//! posture `lux-apple-auth` takes with its Apple key.

mod http;
mod routes;
mod store;
mod tokens;

use std::sync::Arc;

use aws_config::BehaviorVersion;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use lux_pco::{Http, HttpRequest, OAuthApp, ReqwestHttp};
use serde_json::Value;

/// The transport, shared by every client this service builds.
///
/// `lux-pco`'s [`PcoClient`](lux_pco::PcoClient) owns its transport by value,
/// and a warm container serves many requests — so the one connection pool is
/// handed out behind an `Arc` rather than rebuilt per request, which is the
/// difference between a two-second poll and a two-second poll plus a TLS
/// handshake.
#[derive(Clone)]
pub struct SharedHttp(Arc<ReqwestHttp>);

impl Http for SharedHttp {
    fn send(
        &self,
        request: HttpRequest,
    ) -> lux_pco::http::BoxFuture<'_, Result<lux_pco::http::HttpResponse, lux_pco::Error>> {
        self.0.send(request)
    }
}

pub(crate) struct Ctx {
    pub ddb: aws_sdk_dynamodb::Client,
    pub secrets: aws_sdk_secretsmanager::Client,
    pub verifier: lux_auth::Verifier,
    pub http: SharedHttp,
    pub table: String,
    pub secret_id: String,
    /// The registered redirect URI this deployment hands to Planning Center.
    /// Configuration, never a literal at a call site — dev and prod disagree
    /// about it by design, and Planning Center matches it byte for byte.
    pub redirect_uri: String,
    /// The OAuth application, read from Secrets Manager on first use. Lazy so
    /// an unseeded secret is a 503 on two routes rather than a function that
    /// will not start.
    oauth: tokio::sync::OnceCell<OAuthApp>,
}

impl Ctx {
    /// The registered OAuth application, fetched once per warm container.
    pub async fn oauth(&self) -> Result<&OAuthApp, String> {
        self.oauth
            .get_or_try_init(|| load_oauth(&self.secrets, &self.secret_id, &self.redirect_uri))
            .await
    }
}

/// Read and parse the `{client_id, client_secret}` secret.
async fn load_oauth(
    secrets: &aws_sdk_secretsmanager::Client,
    secret_id: &str,
    redirect_uri: &str,
) -> Result<OAuthApp, String> {
    #[derive(serde::Deserialize)]
    struct Stored {
        client_id: String,
        client_secret: String,
    }

    let out = secrets
        .get_secret_value()
        .secret_id(secret_id)
        .send()
        .await
        .map_err(|e| format!("pco oauth secret read failed: {e}"))?;
    let raw = out
        .secret_string()
        .ok_or("pco oauth secret has no string value")?;
    let stored: Stored =
        serde_json::from_str(raw).map_err(|e| format!("pco oauth secret is malformed: {e}"))?;
    if stored.client_id.trim().is_empty() || stored.client_secret.trim().is_empty() {
        return Err("pco oauth secret is missing a credential".into());
    }
    Ok(OAuthApp::new(
        stored.client_id,
        stored.client_secret,
        redirect_uri,
    ))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    // reqwest uses rustls with no baked provider; install ring as the process
    // default before any TLS (the Cognito JWKS fetch below is the first one).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let pool_id = env("COGNITO_USER_POOL_ID")?;
    let client_id = env("COGNITO_APP_CLIENT_ID")?;
    let region = env("COGNITO_REGION")?;
    let table = env("DYNAMODB_TABLE")?;
    let secret_id =
        std::env::var("PCO_SECRET_ID").unwrap_or_else(|_| lux_pco::oauth::SECRET_PATH.to_owned());
    let redirect_uri = std::env::var("PCO_REDIRECT_URI")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| lux_pco::oauth::REDIRECT_URI_PROD.to_owned());

    let verifier = lux_auth::Verifier::new(&region, &pool_id, &client_id)
        .await
        .expect("failed to fetch Cognito JWKS");

    let conf = aws_config::defaults(BehaviorVersion::latest())
        .http_client(aws_http_client())
        .load()
        .await;

    let ctx = Arc::new(Ctx {
        ddb: aws_sdk_dynamodb::Client::new(&conf),
        secrets: aws_sdk_secretsmanager::Client::new(&conf),
        verifier,
        http: SharedHttp(Arc::new(ReqwestHttp::new()?)),
        table,
        secret_id,
        redirect_uri,
        oauth: tokio::sync::OnceCell::new(),
    });

    run(service_fn(move |event: LambdaEvent<Value>| {
        let ctx = ctx.clone();
        async move { http::handle(&ctx, event.payload).await }
    }))
    .await
}

fn env(key: &str) -> Result<String, Error> {
    std::env::var(key).map_err(|_| format!("missing required env var {key}").into())
}

/// The AWS SDK's HTTP client, built explicitly rather than taken from
/// `aws-config`'s default — same reasoning as `lux-apple-auth`: the bundled
/// default drags in a second, older TLS stack for a type nothing here uses.
fn aws_http_client() -> aws_smithy_runtime_api::client::http::SharedHttpClient {
    aws_smithy_http_client::Builder::new()
        .tls_provider(aws_smithy_http_client::tls::Provider::Rustls(
            aws_smithy_http_client::tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https()
}
