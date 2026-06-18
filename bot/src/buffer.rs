use crate::Context;
use crate::Error;
use poise::serenity_prelude::{self as serenity};
use poise::ChoiceParameter;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChoiceParameter)]
enum ColorOption {
    Red,
    Blue,
    Green,
    Amber,
    Daylight,
    White,
}

fn get_color_values(color: ColorOption) -> [u8; 6] {
    let mut color_map = HashMap::new();
    color_map.insert(ColorOption::Red, [255, 0, 0, 0, 0, 255]);
    color_map.insert(ColorOption::Blue, [0, 0, 255, 0, 0, 255]);
    color_map.insert(ColorOption::Green, [0, 255, 0, 0, 0, 255]);
    color_map.insert(ColorOption::Amber, [0, 0, 0, 255, 0, 255]);
    color_map.insert(ColorOption::Daylight, [0, 0, 0, 0, 255, 255]);
    color_map.insert(ColorOption::White, [255, 255, 255, 255, 255, 255]);

    *color_map.get(&color).unwrap_or(&[0, 0, 0, 0, 0, 0])
}

#[poise::command(slash_command)]
pub async fn set_buffer(
    ctx: Context<'_>,
    #[description = "Color of the lights"] color: ColorOption,
) -> Result<(), Error> {
    let ngrok_tunnel_domain = std::env::var("NGROK_TUNNEL_DOMAIN").unwrap();

    let url = format!("https://{}/buffer", ngrok_tunnel_domain);

    let buffer = get_color_values(color);
    let mut map = HashMap::new();
    map.insert("buffer", buffer);
    let client = reqwest::Client::new();

    let response = client
        .post(url)
        .json(&map)
        .send()
        .await
        .map_err(|e| {
            eprintln!("Failed to make request: {}", e);
            serenity::Error::Other("Failed to make request")
        })?
        .text()
        .await
        .map_err(|e| {
            eprintln!("Failed to read response: {}", e);
            serenity::Error::Other("Failed to read response")
        })?;

    ctx.say(response).await?;
    Ok(())
}
