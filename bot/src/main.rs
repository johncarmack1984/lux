//! lux-discord-bot — a Discord Interactions endpoint on AWS Lambda.
//!
//! Discord POSTs interactions to the Lambda's Function URL. We verify the
//! ed25519 request signature, answer the PING handshake, and on
//! `/set_buffer` publish the chosen color to the device's AWS IoT topic
//! (`lux/<id>/buffer/set`) using the Lambda's IAM role — no tunnel, no
//! long-lived keys, single account. The lux app, subscribed to that topic,
//! applies the buffer to the fixture.

mod buffer;

use std::sync::Arc;

use aws_config::BehaviorVersion;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use lambda_http::{run, service_fn, Body, Error, Request, Response};
use serde_json::Value;

struct Ctx {
    iot: aws_sdk_iotdataplane::Client,
    topic: String,
    public_key: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();

    let public_key = std::env::var("DISCORD_PUBLIC_KEY")
        .expect("DISCORD_PUBLIC_KEY (Discord application public key) must be set");
    let endpoint = std::env::var("AWS_IOT_ENDPOINT")
        .expect("AWS_IOT_ENDPOINT (IoT Data-ATS endpoint) must be set");
    let device_id = std::env::var("LUX_DEVICE_ID").unwrap_or_else(|_| "lux-1".into());

    let conf = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let iot = aws_sdk_iotdataplane::Client::from_conf(
        aws_sdk_iotdataplane::config::Builder::from(&conf)
            .endpoint_url(format!("https://{endpoint}"))
            .build(),
    );

    let ctx = Arc::new(Ctx {
        iot,
        topic: format!("lux/{device_id}/buffer/set"),
        public_key,
    });

    run(service_fn(move |req: Request| {
        let ctx = ctx.clone();
        async move { handle(ctx, req).await }
    }))
    .await
}

async fn handle(ctx: Arc<Ctx>, req: Request) -> Result<Response<Body>, Error> {
    let signature = req
        .headers()
        .get("x-signature-ed25519")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let timestamp = req
        .headers()
        .get("x-signature-timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = match req.body() {
        Body::Text(s) => s.clone().into_bytes(),
        Body::Binary(b) => b.clone(),
        Body::Empty => Vec::new(),
    };

    // Discord rejects an endpoint that doesn't enforce this.
    if !verify(&ctx.public_key, &timestamp, &body, &signature) {
        return reply(401, Value::String("invalid request signature".into()));
    }

    let interaction: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    match interaction.get("type").and_then(Value::as_u64) {
        Some(1) => reply(200, serde_json::json!({ "type": 1 })), // PING -> PONG
        Some(2) => reply(200, run_command(&ctx, &interaction).await),
        _ => reply(200, message("Unsupported interaction.")),
    }
}

async fn run_command(ctx: &Ctx, interaction: &Value) -> Value {
    let name = interaction
        .pointer("/data/name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if name != "set_buffer" {
        return message(&format!("Unknown command: {name}"));
    }
    let color = interaction
        .pointer("/data/options/0/value")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(buffer) = buffer::color_to_buffer(color) else {
        return message(&format!("Unknown color: {color}"));
    };
    match buffer::publish(&ctx.iot, &ctx.topic, buffer).await {
        Ok(()) => message(&format!("Set the lights to {color}.")),
        Err(e) => {
            tracing::error!("IoT publish failed: {e}");
            message("Couldn't reach the lights right now.")
        }
    }
}

/// A CHANNEL_MESSAGE_WITH_SOURCE interaction response.
fn message(content: &str) -> Value {
    serde_json::json!({ "type": 4, "data": { "content": content } })
}

fn reply(status: u16, body: Value) -> Result<Response<Body>, Error> {
    Ok(Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))?)
}

/// Verify Discord's ed25519 signature over `timestamp || body`.
fn verify(public_key_hex: &str, timestamp: &str, body: &[u8], signature_hex: &str) -> bool {
    let Ok(pk) = hex::decode(public_key_hex) else {
        return false;
    };
    let Ok(pk) = <[u8; 32]>::try_from(pk.as_slice()) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&pk) else {
        return false;
    };
    let Ok(sig) = hex::decode(signature_hex) else {
        return false;
    };
    let Ok(sig) = <[u8; 64]>::try_from(sig.as_slice()) else {
        return false;
    };
    let signature = Signature::from_bytes(&sig);
    let mut message = timestamp.as_bytes().to_vec();
    message.extend_from_slice(body);
    verifying_key.verify(&message, &signature).is_ok()
}
