# The Planning Center bridge: the lux-plan-bridge Lambda (../services/plan-bridge).
#
# Same shape as the other services — public Function URL, handler-enforced auth,
# least-privilege role, placeholder code owned by cargo-lambda deploys — with one
# addition: its routes are also served through the auth.lux.johncarmack.com
# CloudFront distribution (apple-auth-web.tf), because Planning Center matches a
# registered redirect URI byte for byte and a raw *.lambda-url.on.aws host is not
# one. The registered callback is https://auth.lux.johncarmack.com/pco/callback.
#
# The OAuth client id + secret (`/lux/bridge/prod/pco-oauth`) are a hand-created
# secret, never a Terraform resource — the house pattern for true secrets, and in
# this case a hard requirement: Planning Center shows the client secret exactly
# once, at registration. The function lazy-loads it, so this stack applies and
# serves before the secret exists; only the connect routes need it.

# --- Role: logs, its own two partitions, the OAuth secret ---------------------

resource "aws_iam_role" "lux_plan_bridge" {
  name = "lux-plan-bridge"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "lambda.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lux_plan_bridge_logs" {
  role       = aws_iam_role.lux_plan_bridge.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

# The connection items live in the lux-sync table but in their own partitions;
# LeadingKeys pins this role to exactly those, so a bug here can never reach a
# user's setups, settings, or Apple links. PCO# = the church's tokens,
# PCOSTATE# = in-flight connect attempts (self-expiring via the table's `ttl`).
# These prefixes are asserted in services/plan-bridge/src/store.rs.
resource "aws_iam_role_policy" "lux_plan_bridge_ddb" {
  name = "lux-plan-bridge-dynamodb"
  role = aws_iam_role.lux_plan_bridge.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "dynamodb:GetItem",
        "dynamodb:PutItem",
        "dynamodb:UpdateItem",
        "dynamodb:DeleteItem",
      ]
      Resource = aws_dynamodb_table.lux_sync.arn
      Condition = {
        "ForAllValues:StringLike" = {
          "dynamodb:LeadingKeys" = ["PCO#*", "PCOSTATE#*"]
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "lux_plan_bridge_pco_secret" {
  name = "lux-plan-bridge-pco-secret"
  role = aws_iam_role.lux_plan_bridge.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = "arn:aws:secretsmanager:*:${local.aws_account_id}:secret:/lux/bridge/prod/pco-oauth*"
    }]
  })
}

resource "aws_cloudwatch_log_group" "lux_plan_bridge" {
  name              = "/aws/lambda/lux-plan-bridge"
  retention_in_days = 14
}

# --- Function + URL (placeholder code; cargo-lambda ships the real thing) -----

data "archive_file" "plan_bridge_placeholder" {
  type        = "zip"
  output_path = "${path.module}/.plan-bridge-placeholder.zip"
  source {
    content  = "placeholder — real code is shipped by cargo-lambda"
    filename = "bootstrap"
  }
}

resource "aws_lambda_function" "lux_plan_bridge" {
  function_name = "lux-plan-bridge"
  role          = aws_iam_role.lux_plan_bridge.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = ["x86_64"]
  memory_size   = 256
  # A plan read is two upstream calls (next plan, then its items), each with a
  # 5s client timeout, plus a possible token refresh. Ten would be too tight on
  # the one Sunday Planning Center is slow.
  timeout = 20

  filename = data.archive_file.plan_bridge_placeholder.output_path
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }

  environment {
    variables = {
      COGNITO_USER_POOL_ID  = aws_cognito_user_pool.lux.id
      COGNITO_APP_CLIENT_ID = aws_cognito_user_pool_client.lux_app.id
      COGNITO_REGION        = data.aws_region.current.region
      DYNAMODB_TABLE        = aws_dynamodb_table.lux_sync.name
      # By-name reference to the hand-created secret (see the header comment).
      PCO_SECRET_ID = "/lux/bridge/prod/pco-oauth"
      # The redirect URI registered on the Planning Center OAuth application.
      # Product identity, not a secret — and it must match the registration
      # byte for byte, which is why it is config rather than a literal in code.
      PCO_REDIRECT_URI = "https://${local.apple_auth_domain}/pco/callback"
    }
  }
}

# Public Function URL; the handler enforces auth (Cognito bearer on every route
# but the callback, which authenticates on its single-use `state`) — same model
# as the sync API and the Apple bridge.
resource "aws_lambda_function_url" "lux_plan_bridge" {
  function_name      = aws_lambda_function.lux_plan_bridge.function_name
  authorization_type = "NONE"
}

resource "aws_lambda_permission" "lux_plan_bridge_url" {
  statement_id           = "FunctionURLAllowPublicAccess"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.lux_plan_bridge.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}

# Consumed by scripts/gen-endpoints: the app's `planBridgeUrl`. Absent field ⇒
# the /plan route stays dark, so adding this output does not touch the drift
# gate until the endpoints file is regenerated.
output "plan_bridge_url" {
  description = "PLAN_BRIDGE_URL — the Planning Center bridge the app reads plans through."
  value       = aws_lambda_function_url.lux_plan_bridge.function_url
}
