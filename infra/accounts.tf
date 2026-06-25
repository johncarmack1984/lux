# Accounts + cloud-synced setups: a Cognito user pool for identity and a
# DynamoDB table for per-user setup state, fronted by the lux-sync-api Lambda
# (../sync-api). Same shape as the IoT remote-control path — single account,
# nothing always-on, no long-lived keys: the app holds only a short-lived
# Cognito JWT, never AWS credentials. (aws_region/aws_caller_identity data
# sources are declared in iot.tf and shared across the module.)

# --- Identity: Cognito user pool ---

resource "aws_cognito_user_pool" "lux" {
  name                     = "lux"
  username_attributes      = ["email"]
  auto_verified_attributes = ["email"]

  password_policy {
    minimum_length    = 8
    require_lowercase = true
    require_numbers   = true
    require_uppercase = true
    require_symbols   = false
  }

  account_recovery_setting {
    recovery_mechanism {
      name     = "verified_email"
      priority = 1
    }
  }

  # COGNITO_DEFAULT caps daily email volume low — fine for now; move to SES
  # before real signup volume.
  email_configuration {
    email_sending_account = "COGNITO_DEFAULT"
  }
}

# Public/native app client — a desktop app can't keep a secret. SRP (the
# password never crosses the wire) + refresh-token auth; no hosted-UI OAuth.
resource "aws_cognito_user_pool_client" "lux_app" {
  name         = "lux-app"
  user_pool_id = aws_cognito_user_pool.lux.id

  generate_secret = false
  explicit_auth_flows = [
    "ALLOW_USER_SRP_AUTH",            # the app's flow — password never crosses the wire
    "ALLOW_REFRESH_TOKEN_AUTH",       # silent re-auth from the stored refresh token
    "ALLOW_ADMIN_USER_PASSWORD_AUTH", # admin-only (needs AWS creds), for minting test tokens via the CLI
  ]

  access_token_validity  = 1
  id_token_validity      = 1
  refresh_token_validity = 30
  token_validity_units {
    access_token  = "hours"
    id_token      = "hours"
    refresh_token = "days"
  }

  prevent_user_existence_errors = "ENABLED"
}

# --- State: DynamoDB single table (one item per setup) ---

resource "aws_dynamodb_table" "lux_sync" {
  name         = "lux-sync"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "pk"
  range_key    = "sk"

  attribute {
    name = "pk"
    type = "S"
  }
  attribute {
    name = "sk"
    type = "S"
  }

  # Off for the initial scale-to-zero stand-up — PITR is billed on table data
  # size. Flip to true once there's real user data worth the 35-day restore
  # window (it can be enabled live, no table rebuild).
  point_in_time_recovery {
    enabled = false
  }
}

# --- Sync API: lux-sync-api Lambda (../sync-api) + Function URL ---

# Least-privilege role: write logs, and touch only the lux-sync table.
resource "aws_iam_role" "lux_sync_api" {
  name = "lux-sync-api"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "lambda.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lux_sync_api_logs" {
  role       = aws_iam_role.lux_sync_api.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_iam_role_policy" "lux_sync_api_ddb" {
  name = "lux-sync-api-dynamodb"
  role = aws_iam_role.lux_sync_api.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "dynamodb:Query",
        "dynamodb:GetItem",
        "dynamodb:PutItem",
        "dynamodb:UpdateItem",
      ]
      Resource = aws_dynamodb_table.lux_sync.arn
    }]
  })
}

# Own the log group explicitly with a finite retention so logs self-clean —
# Lambda's auto-created group defaults to never-expire. Name must match
# /aws/lambda/<function-name> so the function writes here.
resource "aws_cloudwatch_log_group" "lux_sync_api" {
  name              = "/aws/lambda/lux-sync-api"
  retention_in_days = 14
}

# Terraform owns the function's *config*; cargo-lambda ships the code. A tiny
# placeholder package lets `apply` create the function before the first deploy,
# and ignore_changes keeps later deploys from being reverted (mirrors the bot
# in generated.tf).
data "archive_file" "sync_api_placeholder" {
  type        = "zip"
  output_path = "${path.module}/.sync-api-placeholder.zip"
  source {
    content  = "placeholder — real code is shipped by cargo-lambda"
    filename = "bootstrap"
  }
}

resource "aws_lambda_function" "lux_sync_api" {
  function_name = "lux-sync-api"
  role          = aws_iam_role.lux_sync_api.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = ["x86_64"]
  memory_size   = 256
  timeout       = 10

  filename = data.archive_file.sync_api_placeholder.output_path
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }

  environment {
    variables = {
      COGNITO_USER_POOL_ID  = aws_cognito_user_pool.lux.id
      COGNITO_APP_CLIENT_ID = aws_cognito_user_pool_client.lux_app.id
      COGNITO_REGION        = data.aws_region.current.region
      DYNAMODB_TABLE        = aws_dynamodb_table.lux_sync.name
    }
  }
}

# Public Function URL; the handler enforces auth by verifying the Cognito JWT
# (same model as the bot's signature check), so no IAM auth on the URL itself.
resource "aws_lambda_function_url" "lux_sync_api" {
  function_name      = aws_lambda_function.lux_sync_api.function_name
  authorization_type = "NONE"
}

resource "aws_lambda_permission" "lux_sync_api_url" {
  statement_id           = "FunctionURLAllowPublicAccess"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.lux_sync_api.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}

# --- Outputs: write these into the app's env (src-tauri/.env) ---

output "cognito_user_pool_id" {
  description = "COGNITO_USER_POOL_ID for the app."
  value       = aws_cognito_user_pool.lux.id
}

output "cognito_app_client_id" {
  description = "COGNITO_APP_CLIENT_ID for the app (public client, no secret)."
  value       = aws_cognito_user_pool_client.lux_app.id
}

output "cognito_region" {
  description = "COGNITO_REGION for the app."
  value       = data.aws_region.current.region
}

output "lux_sync_url" {
  description = "LUX_SYNC_URL — the sync API base the app pushes/pulls setups to."
  value       = aws_lambda_function_url.lux_sync_api.function_url
}

output "dynamodb_table_name" {
  description = "Name of the per-user setup-state table."
  value       = aws_dynamodb_table.lux_sync.name
}
