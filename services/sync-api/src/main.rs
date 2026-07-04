//! lux-sync-api — cloud sync for the lux desktop app, on AWS Lambda.
//!
//! The app calls this Function URL with `Authorization: Bearer <Cognito ID
//! token>` to push and pull its setups. The handler verifies the JWT against the
//! Cognito user pool (`lux-auth`), derives the caller's `sub`, and reads/writes
//! only that user's partition of the `lux-sync` DynamoDB table (`store`). No AWS
//! credentials ever reach the client, and cross-tenant access is impossible: the
//! partition key comes from the verified token, never from the request.
//!
//! Every body that crosses this wire is a `lux-wire` type — the same crate the
//! desktop deserializes with — so the two sides cannot drift.
//!
//! Routes (all under the Function URL):
//! - `GET    /setups`        — list the caller's setups (incl. tombstones)
//! - `PUT    /setups/{id}`   — create/update one, optimistic-concurrency on `baseUpdatedAt`
//! - `DELETE /setups/{id}?baseUpdatedAt=N` — soft-delete (tombstone)
//! - `DELETE /user`          — hard-delete the caller's whole partition (account deletion)
//!
//! After each committed write the handler publishes a tiny opaque nudge frame
//! to the caller's own IoT topic (`lux_wire::nudge`) so their other devices
//! pull promptly. Publishing is best-effort and never fails the request; with
//! `IOT_ENDPOINT` unset (tests, minimal dev stacks) it is skipped entirely.

mod store;

use std::sync::Arc;

use aws_config::BehaviorVersion;
use lambda_http::{run, service_fn, Body, Error, Request, RequestExt, Response};
use lux_wire::{
    DeleteUserDataResponse, ErrorResponse, ListSetupsResponse, TombstoneResponse, UpsertSetupBody,
    WriteResponse,
};
use serde::{Deserialize, Serialize};

use store::StoreError;

struct Ctx {
    ddb: aws_sdk_dynamodb::Client,
    table: String,
    verifier: lux_auth::Verifier,
    /// IoT data-plane client for change nudges; `None` when `IOT_ENDPOINT` is unset.
    iot: Option<aws_sdk_iotdataplane::Client>,
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

    let pool_id = env("COGNITO_USER_POOL_ID");
    let client_id = env("COGNITO_APP_CLIENT_ID");
    let region = env("COGNITO_REGION");
    let table = env("DYNAMODB_TABLE");

    let verifier = lux_auth::Verifier::new(&region, &pool_id, &client_id)
        .await
        .expect("failed to fetch Cognito JWKS");

    let conf = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&conf);

    // The nudge publisher needs the account's ATS data endpoint (the default
    // SDK endpoint is the wrong hostname class for IoT data-plane calls).
    let iot = std::env::var("IOT_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|host| {
            let url = if host.starts_with("http") {
                host
            } else {
                format!("https://{host}")
            };
            let iot_conf = aws_sdk_iotdataplane::config::Builder::from(&conf)
                .endpoint_url(url)
                .build();
            aws_sdk_iotdataplane::Client::from_conf(iot_conf)
        });
    if iot.is_none() {
        tracing::info!("IOT_ENDPOINT unset; change nudges disabled");
    }

    let ctx = Arc::new(Ctx {
        ddb,
        table,
        verifier,
        iot,
    });

    run(service_fn(move |req: Request| {
        let ctx = ctx.clone();
        async move { handle(ctx, req).await }
    }))
    .await
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} must be set"))
}

async fn handle(ctx: Arc<Ctx>, req: Request) -> Result<Response<Body>, Error> {
    // Identity comes only from the verified token; the request never names a user.
    let sub = match bearer(&req).and_then(|t| ctx.verifier.verify(&t).ok()) {
        Some(claims) => claims.sub,
        None => return reply(401, error("invalid or missing token")),
    };

    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();

    match (method.as_str(), segments.as_slice()) {
        ("GET", [seg]) if *seg == lux_wire::SETUPS_SEGMENT => list(&ctx, &sub).await,
        ("PUT", [seg, id]) if *seg == lux_wire::SETUPS_SEGMENT => {
            upsert(&ctx, &sub, id, &req).await
        }
        ("DELETE", [seg, id]) if *seg == lux_wire::SETUPS_SEGMENT => {
            tombstone(&ctx, &sub, id, &req).await
        }
        ("DELETE", [seg]) if *seg == lux_wire::USER_SEGMENT => delete_user_data(&ctx, &sub).await,
        _ => reply(404, error("not found")),
    }
}

async fn list(ctx: &Ctx, sub: &str) -> Result<Response<Body>, Error> {
    match store::list(&ctx.ddb, &ctx.table, sub).await {
        Ok(setups) => reply(200, ListSetupsResponse { setups }),
        Err(e) => {
            tracing::error!("list failed: {e}");
            reply(500, error("internal"))
        }
    }
}

async fn upsert(ctx: &Ctx, sub: &str, id: &str, req: &Request) -> Result<Response<Body>, Error> {
    let body: UpsertSetupBody = match parse_body(req) {
        Ok(b) => b,
        Err(e) => return reply(400, error(&e)),
    };
    match store::upsert(&ctx.ddb, &ctx.table, sub, id, &body, now_millis()).await {
        Ok(res) => {
            nudge(ctx, sub).await;
            reply(
                200,
                WriteResponse {
                    updated_at: res.updated_at,
                    rev: res.rev,
                },
            )
        }
        Err(StoreError::Conflict) => reply(409, error("conflict")),
        Err(StoreError::Internal(e)) => {
            tracing::error!("upsert failed: {e}");
            reply(500, error("internal"))
        }
    }
}

async fn tombstone(ctx: &Ctx, sub: &str, id: &str, req: &Request) -> Result<Response<Body>, Error> {
    let base = req
        .query_string_parameters()
        .first(lux_wire::BASE_UPDATED_AT_QUERY)
        .and_then(|s| s.parse::<i64>().ok());
    match store::tombstone(&ctx.ddb, &ctx.table, sub, id, base, now_millis()).await {
        Ok(updated_at) => {
            nudge(ctx, sub).await;
            reply(
                200,
                TombstoneResponse {
                    updated_at,
                    deleted: true,
                },
            )
        }
        Err(StoreError::Conflict) => reply(409, error("conflict")),
        Err(StoreError::Internal(e)) => {
            tracing::error!("tombstone failed: {e}");
            reply(500, error("internal"))
        }
    }
}

/// Account deletion, step 1 of 2: hard-delete everything the caller owns. The
/// app calls this while the tokens still authenticate, then removes the Cognito
/// user itself (self-service `DeleteUser`). No nudge: the other devices' next
/// refresh fails and they simply sign out.
async fn delete_user_data(ctx: &Ctx, sub: &str) -> Result<Response<Body>, Error> {
    match store::delete_all(&ctx.ddb, &ctx.table, sub).await {
        Ok(deleted_items) => reply(200, DeleteUserDataResponse { deleted_items }),
        Err(e) => {
            tracing::error!("account data wipe failed: {e}");
            reply(500, error("internal"))
        }
    }
}

/// Best-effort change nudge: publish the opaque `{"changed":"setups"}` frame to
/// the caller's own topic so their other devices pull now. Never fails the
/// request — a missed nudge is healed by the app's pull-on-focus/reconnect
/// safety nets. (All of the user's connected devices get the frame, including
/// the writer; its re-pull is coalesced client-side and harmless.)
async fn nudge(ctx: &Ctx, sub: &str) {
    let Some(iot) = &ctx.iot else { return };
    let topic = lux_wire::nudge::user_topic(sub);
    if let Err(e) = iot
        .publish()
        .topic(&topic)
        .qos(0)
        .payload(aws_sdk_iotdataplane::primitives::Blob::new(
            lux_wire::nudge::setups_changed_frame().into_bytes(),
        ))
        .send()
        .await
    {
        tracing::warn!("nudge publish to {topic} failed: {e}");
    }
}

// --- request/response helpers -----------------------------------------------

/// The bearer token from the `Authorization` header, if present and well-formed.
fn bearer(req: &Request) -> Option<String> {
    req.headers()
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_owned)
}

fn body_bytes(req: &Request) -> Vec<u8> {
    match req.body() {
        Body::Text(s) => s.clone().into_bytes(),
        Body::Binary(b) => b.clone(),
        // `Body` is #[non_exhaustive]; treat anything else (incl. Empty) as no body.
        _ => Vec::new(),
    }
}

fn parse_body<T: for<'de> Deserialize<'de>>(req: &Request) -> Result<T, String> {
    serde_json::from_slice(&body_bytes(req)).map_err(|e| format!("bad body: {e}"))
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn error(message: &str) -> ErrorResponse {
    ErrorResponse {
        error: message.to_owned(),
    }
}

fn reply<T: Serialize>(status: u16, body: T) -> Result<Response<Body>, Error> {
    Ok(Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body)?))?)
}
