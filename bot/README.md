# lux-bot

A Discord [Interactions](https://discord.com/developers/docs/interactions/receiving-and-responding) endpoint for lux, deployed as an AWS Lambda (`provided.al2023`) behind a Function URL. Discord POSTs slash-command interactions to the Function URL; the Lambda verifies the ed25519 request signature and, for `/set_buffer`, publishes the chosen color to the device's AWS IoT topic. The lux desktop app subscribes to that topic and drives the fixture — so there is no public ingress to the machine running the lights, no tunnel, and no long-lived credentials (the Lambda publishes via its IAM role, all in one AWS account).

This replaces the previous Shuttle/poise gateway bot, which reached the app over an ngrok tunnel.

## Slash command

`/set_buffer color:<red | blue | green | amber | daylight | white>` publishes `{ "buffer": [r, g, b, a, w, master] }` to `lux/<device>/buffer/set`.

## Environment

| Variable | Purpose |
|---|---|
| `DISCORD_PUBLIC_KEY` | Discord application public key; verifies the request signature. |
| `AWS_IOT_ENDPOINT` | IoT Data-ATS endpoint (`xxxxxxxx-ats.iot.us-west-1.amazonaws.com`); output by Terraform in `../infra`. |
| `LUX_DEVICE_ID` | Device segment of the topic; defaults to `lux-1` and must match the lux app's `LUX_DEVICE_ID`. |

## Build & deploy

```bash
cargo lambda build --release --x86-64   # match the imported function's architecture
cargo lambda deploy lux-discord-bot      # config (role, env, Function URL) is managed in ../infra
```

Then set the function's Function URL as the **Interactions Endpoint URL** in the Discord developer portal; Discord sends a signed PING that this handler answers.

## Register the slash command (one-time)

```bash
curl -X PUT \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
  -H "Content-Type: application/json" \
  "https://discord.com/api/v10/applications/$DISCORD_APPLICATION_ID/commands" \
  -d '[{
        "name": "set_buffer",
        "description": "Set the lux lights to a color",
        "options": [{
          "name": "color", "description": "Color of the lights", "type": 3, "required": true,
          "choices": [
            {"name": "Red", "value": "red"}, {"name": "Blue", "value": "blue"},
            {"name": "Green", "value": "green"}, {"name": "Amber", "value": "amber"},
            {"name": "Daylight", "value": "daylight"}, {"name": "White", "value": "white"}
          ]
        }]
      }]'
```
