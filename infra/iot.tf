# Remote-control path: the lux-discord-bot Lambda publishes buffer commands to
# AWS IoT Core; the lux desktop app subscribes over mutual TLS and drives the
# fixture. Single account, no public ingress to the device, no long-lived keys.

variable "device_id" {
  description = "Topic/thing segment for the lux device; must match the app and bot LUX_DEVICE_ID."
  type        = string
  default     = "lux-1"
}

variable "discord_public_key" {
  description = "Discord application public key (hex); the bot verifies request signatures with it."
  type        = string
}

data "aws_caller_identity" "current" {}
data "aws_region" "current" {}

# Account/region-specific MQTT endpoint the device and bot connect to.
data "aws_iot_endpoint" "ats" {
  endpoint_type = "iot:Data-ATS"
}

locals {
  control_topic   = "lux/${var.device_id}/buffer/set"
  arn_prefix      = "arn:aws:iot:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}"
  topic_arn       = "${local.arn_prefix}:topic/${local.control_topic}"
  topicfilter_arn = "${local.arn_prefix}:topicfilter/${local.control_topic}"
  client_arn      = "${local.arn_prefix}:client/${var.device_id}"
}

# --- The device (lux desktop app) ---

resource "aws_iot_thing" "lux" {
  name = var.device_id
}

resource "aws_iot_certificate" "lux" {
  active = true
}

# The device may connect as its client id and subscribe to its own topic only.
resource "aws_iot_policy" "lux_device" {
  name = "lux-device-${var.device_id}"
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = "iot:Connect"
        Resource = local.client_arn
      },
      {
        Effect   = "Allow"
        Action   = "iot:Subscribe"
        Resource = local.topicfilter_arn
      },
      {
        Effect   = "Allow"
        Action   = "iot:Receive"
        Resource = local.topic_arn
      },
    ]
  })
}

resource "aws_iot_policy_attachment" "lux" {
  policy = aws_iot_policy.lux_device.name
  target = aws_iot_certificate.lux.arn
}

resource "aws_iot_thing_principal_attachment" "lux" {
  thing     = aws_iot_thing.lux.name
  principal = aws_iot_certificate.lux.arn
}

# --- The bot (Lambda) may publish commands to the device topic ---

resource "aws_iam_role_policy" "bot_iot_publish" {
  name = "lux-bot-iot-publish"
  role = aws_iam_role.cargo_lambda_role.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = "iot:Publish"
      Resource = local.topic_arn
    }]
  })
}

# --- Device provisioning material: write these to the app's .env ---

output "iot_endpoint" {
  description = "AWS_IOT_ENDPOINT for both the app and the bot."
  value       = data.aws_iot_endpoint.ats.endpoint_address
}

output "bot_function_url" {
  description = "Set this as the Discord application's Interactions Endpoint URL."
  value       = aws_lambda_function_url.lux_discord_bot.function_url
}

output "device_certificate_pem" {
  description = "Device certificate; write to the file referenced by the app's AWS_IOT_CERT_PATH."
  value       = aws_iot_certificate.lux.certificate_pem
  sensitive   = true
}

output "device_private_key" {
  description = "Device private key; write to the file referenced by AWS_IOT_KEY_PATH. (Also stored in TF state.)"
  value       = aws_iot_certificate.lux.private_key
  sensitive   = true
}
