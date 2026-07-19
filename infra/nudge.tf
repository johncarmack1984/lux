# Change-nudge channel: after each committed write the lux-sync-api Lambda
# publishes a tiny opaque frame to the writer's own topic (lux/sync/user/<sub>);
# the desktop keeps an open MQTT-over-WebSocket connection to IoT Core and pulls
# on any frame. IoT Core is the connection holder — the serverless stand-in for
# vegify's standing server in the house sync model — and the custom authorizer
# below verifies the app's Cognito ID token and scopes each connection to the
# verified user's own topic: the same token-derived tenant isolation as the
# sync-api's DynamoDB partition key.

# --- The authorizer Lambda (services/iot-authorizer) ---

# Least-privilege role: logs only — the authorizer just verifies a JWT and
# returns a policy document.
resource "aws_iam_role" "lux_iot_authorizer" {
  name = "lux-iot-authorizer"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "lambda.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lux_iot_authorizer_logs" {
  role       = aws_iam_role.lux_iot_authorizer.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_cloudwatch_log_group" "lux_iot_authorizer" {
  name              = "/aws/lambda/lux-iot-authorizer"
  retention_in_days = 14
}

# Terraform owns the function's *config*; cargo-lambda ships the code (the same
# placeholder + ignore_changes pattern as the bot and sync-api).
data "archive_file" "iot_authorizer_placeholder" {
  type        = "zip"
  output_path = "${path.module}/.iot-authorizer-placeholder.zip"
  source {
    content  = "placeholder — real code is shipped by cargo-lambda"
    filename = "bootstrap"
  }
}

resource "aws_lambda_function" "lux_iot_authorizer" {
  function_name = "lux-iot-authorizer"
  role          = aws_iam_role.lux_iot_authorizer.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = ["x86_64"]
  memory_size   = 128
  timeout       = 5

  filename = data.archive_file.iot_authorizer_placeholder.output_path
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }

  environment {
    variables = {
      COGNITO_USER_POOL_ID = aws_cognito_user_pool.lux.id
      # Comma-separated: interactive sessions (lux-app) and paired devices
      # (lux-node-device) both connect to the realtime channel.
      COGNITO_APP_CLIENT_ID = "${aws_cognito_user_pool_client.lux_app.id},${aws_cognito_user_pool_client.lux_node_device.id}"
      COGNITO_REGION        = data.aws_region.current.region
      # Shared-control grants, read at connect time (shares.tf grants the
      # LeadingKeys-pinned Query). Unset would simply mean no connection is
      # ever widened past its own owner's space.
      DYNAMODB_TABLE = aws_dynamodb_table.lux_sync.name
    }
  }
}

# Deploys publish a numbered version, smoke that exact version, then repoint
# this alias — the authorizer registration below invokes through it, so a bad
# build never takes auth traffic and rollback is one command:
#   aws lambda update-alias --function-name lux-iot-authorizer \
#     --name live --function-version <prev>
# Terraform seeds the pointer ($LATEST = today's semantics) and then leaves it
# to the deploy pipeline, same hands-off pattern as the placeholder code.
resource "aws_lambda_alias" "lux_iot_authorizer_live" {
  name             = "live"
  function_name    = aws_lambda_function.lux_iot_authorizer.function_name
  function_version = "$LATEST"
  lifecycle {
    ignore_changes = [function_version]
  }
}

# IoT (not a user) invokes the authorizer. Two permissions during the alias
# transition: the unqualified one predates the alias and stays so there is no
# destroy/create window in which IoT can't invoke; the qualified one is what
# the alias-routed registration actually uses.
resource "aws_lambda_permission" "lux_iot_authorizer_invoke" {
  statement_id  = "AllowIoTCustomAuthorizer"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.lux_iot_authorizer.function_name
  principal     = "iot.amazonaws.com"
  source_arn    = aws_iot_authorizer.lux_sync.arn
}

resource "aws_lambda_permission" "lux_iot_authorizer_invoke_alias" {
  statement_id  = "AllowIoTCustomAuthorizerAlias"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.lux_iot_authorizer.function_name
  qualifier     = aws_lambda_alias.lux_iot_authorizer_live.name
  principal     = "iot.amazonaws.com"
  source_arn    = aws_iot_authorizer.lux_sync.arn
}

# --- The authorizer registration ---

# Signing is disabled: the desktop is a public client and can't hold a signing
# key, so the Lambda's JWT verification is the gate — the same posture as the
# sync-api's public Function URL with in-handler JWT. Two protocol literals are
# mirrored from lux-wire (rename = deliberate two-file change): `name` must
# match lux_wire::nudge::AUTHORIZER_NAME and token_key_name must match
# lux_wire::nudge::TOKEN_KEY (the app's handshake header).
resource "aws_iot_authorizer" "lux_sync" {
  name                    = "lux-sync-auth"
  authorizer_function_arn = aws_lambda_alias.lux_iot_authorizer_live.arn
  signing_disabled        = true
  status                  = "ACTIVE"
  token_key_name          = "x-lux-token"
}

# --- The sync-api may publish nudges to any user's topic ---

resource "aws_iam_role_policy" "lux_sync_api_nudge" {
  name = "lux-sync-api-iot-nudge"
  role = aws_iam_role.lux_sync_api.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = "iot:Publish"
      Resource = "${local.arn_prefix}:topic/lux/sync/user/*"
    }]
  })
}

# No outputs here: the app-facing endpoint values flow through
# scripts/gen-endpoints (the nudge endpoint is iot.tf's `iot_endpoint` output —
# the same ATS host), and the authorizer name is protocol, fixed in
# lux_wire::nudge::AUTHORIZER_NAME.
