//! The `/set_buffer` color choices and the AWS IoT publish that drives the
//! lux device.

use aws_sdk_iotdataplane::primitives::Blob;
use aws_sdk_iotdataplane::Client;
use lambda_http::Error;

/// `[red, green, blue, amber, white, master]` — the six DMX channels of the
/// RGBAW fixture lux drives.
pub type Buffer = [u8; 6];

/// Map a Discord choice value to a buffer; `None` for an unknown choice.
pub fn color_to_buffer(color: &str) -> Option<Buffer> {
    Some(match color {
        "red" => [255, 0, 0, 0, 0, 255],
        "blue" => [0, 0, 255, 0, 0, 255],
        "green" => [0, 255, 0, 0, 0, 255],
        "amber" => [0, 0, 0, 255, 0, 255],
        "daylight" => [0, 0, 0, 0, 255, 255],
        "white" => [255, 255, 255, 255, 255, 255],
        _ => return None,
    })
}

/// Publish `{ "buffer": [...] }` to the device's control topic. The lux app,
/// subscribed to that topic, applies it to the fixture.
pub async fn publish(client: &Client, topic: &str, buffer: Buffer) -> Result<(), Error> {
    let payload = serde_json::json!({ "buffer": buffer }).to_string();
    client
        .publish()
        .topic(topic)
        .qos(1)
        .payload(Blob::new(payload.into_bytes()))
        .send()
        .await?;
    Ok(())
}
