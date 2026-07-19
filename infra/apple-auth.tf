# Sign in with Apple: the lux-apple-auth Lambda (../services/apple-auth) — one
# function serving both its Function URL routes (the desktop posts the native
# sheet's identity token to /auth/apple[/link|/revoke]) and the user pool's
# CUSTOM_AUTH trigger events (wired in accounts.tf). Same shape as the sync
# API: public URL, handler-enforced auth, least-privilege role, placeholder
# code owned by cargo-lambda deploys.
#
# The Apple-side signing key (`lux/siwa-key`) is a hand-created secret, never a
# Terraform resource — the house pattern for true secrets (like
# lux/apple-signing). The function references it by name and lazy-loads it, so
# this stack applies and serves token verification before the secret exists;
# only the routes that call Apple's token/revoke endpoints need it.

# --- Role: logs, the link items, pool admin auth, the Apple signing key ------

resource "aws_iam_role" "lux_apple_auth" {
  name = "lux-apple-auth"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "lambda.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lux_apple_auth_logs" {
  role       = aws_iam_role.lux_apple_auth.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

# The Apple↔Cognito link items live in the lux-sync table but in their own
# partitions; LeadingKeys pins this role to exactly those, so it can never
# touch a user's sync data.
resource "aws_iam_role_policy" "lux_apple_auth_ddb" {
  name = "lux-apple-auth-dynamodb"
  role = aws_iam_role.lux_apple_auth.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "dynamodb:GetItem",
        "dynamodb:PutItem",
        "dynamodb:UpdateItem",
        "dynamodb:DeleteItem",
        "dynamodb:ConditionCheckItem",
        # The pairing approve screen's same-egress pending list (PAIRIP#).
        "dynamodb:Query",
      ]
      Resource = aws_dynamodb_table.lux_sync.arn
      Condition = {
        "ForAllValues:StringLike" = {
          # APPLE#/APPLELINK# = Sign in with Apple links; PAIR#/PAIRIP#/DEVICE#
          # = device-pairing records (docs/claim-code-pairing.md — this service
          # grows the /auth/device/* routes).
          "dynamodb:LeadingKeys" = ["APPLE#*", "APPLELINK#*", "PAIR#*", "PAIRIP#*", "DEVICE#*"]
        }
      }
    }]
  })
}

# The admin half of the CUSTOM_AUTH dance: look up / create the user, start the
# flow, answer the challenge. Trust still lives in the pool's Verify trigger
# (this same binary) — these calls add reach, not authority.
resource "aws_iam_role_policy" "lux_apple_auth_cognito" {
  name = "lux-apple-auth-cognito"
  role = aws_iam_role.lux_apple_auth.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "cognito-idp:ListUsers",
        "cognito-idp:AdminCreateUser",
        # Admin-created users land in FORCE_CHANGE_PASSWORD; a discarded random
        # password (set permanent) moves them to CONFIRMED so auth can proceed.
        "cognito-idp:AdminSetUserPassword",
        # Apple verifying an email confirms a stalled self-signup for it.
        "cognito-idp:AdminConfirmSignUp",
        "cognito-idp:AdminInitiateAuth",
        "cognito-idp:AdminRespondToAuthChallenge",
      ]
      Resource = aws_cognito_user_pool.lux.arn
    }]
  })
}

resource "aws_iam_role_policy" "lux_apple_auth_siwa_key" {
  name = "lux-apple-auth-siwa-key"
  role = aws_iam_role.lux_apple_auth.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = "arn:aws:secretsmanager:*:${local.aws_account_id}:secret:lux/siwa-key*"
    }]
  })
}

resource "aws_cloudwatch_log_group" "lux_apple_auth" {
  name              = "/aws/lambda/lux-apple-auth"
  retention_in_days = 14
}

# --- Function + URL (placeholder code; cargo-lambda ships the real thing) ----

data "archive_file" "apple_auth_placeholder" {
  type        = "zip"
  output_path = "${path.module}/.apple-auth-placeholder.zip"
  source {
    content  = "placeholder — real code is shipped by cargo-lambda"
    filename = "bootstrap"
  }
}

resource "aws_lambda_function" "lux_apple_auth" {
  function_name = "lux-apple-auth"
  role          = aws_iam_role.lux_apple_auth.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = ["x86_64"]
  memory_size   = 256
  timeout       = 10

  filename = data.archive_file.apple_auth_placeholder.output_path
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }

  environment {
    variables = {
      COGNITO_USER_POOL_ID  = aws_cognito_user_pool.lux.id
      COGNITO_APP_CLIENT_ID = aws_cognito_user_pool_client.lux_app.id
      # Single ids on purpose (they feed AdminInitiateAuth, not just the
      # verifier): the interactive client above, the device-pairing client
      # here. The trigger pins each answer kind to its client.
      COGNITO_DEVICE_CLIENT_ID = aws_cognito_user_pool_client.lux_node_device.id
      COGNITO_REGION           = data.aws_region.current.region
      DYNAMODB_TABLE           = aws_dynamodb_table.lux_sync.name
      # Product identity, not environment: the bundle id is the token audience
      # and Apple client_id for native flows.
      APPLE_BUNDLE_ID = "com.johncarmack.lux"
      # By-name reference to the hand-created secret (see the header comment).
      SIWA_SECRET_ID = "lux/siwa-key"
    }
  }
}

# Public Function URL; the handler enforces auth (Apple identity token on
# /auth/apple, Cognito bearer on /link and /revoke) — same model as the sync API.
resource "aws_lambda_function_url" "lux_apple_auth" {
  function_name      = aws_lambda_function.lux_apple_auth.function_name
  authorization_type = "NONE"
}

resource "aws_lambda_permission" "lux_apple_auth_url" {
  statement_id           = "FunctionURLAllowPublicAccess"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.lux_apple_auth.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}

# Let the user pool invoke the trigger surface of the same function.
resource "aws_lambda_permission" "lux_apple_auth_cognito" {
  statement_id  = "AllowCognitoTriggerInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.lux_apple_auth.function_name
  principal     = "cognito-idp.amazonaws.com"
  source_arn    = aws_cognito_user_pool.lux.arn
}

# Consumed by scripts/gen-endpoints in a follow-up: the endpoints file gains
# `appleAuthUrl` only once this URL exists in applied state (the missing-field
# case is "feature dark" in the app by design), so adding the output here does
# not touch the drift gate.
output "apple_auth_url" {
  description = "APPLE_AUTH_URL — the Sign in with Apple bridge the app posts identity tokens to."
  value       = aws_lambda_function_url.lux_apple_auth.function_url
}
