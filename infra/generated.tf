# __generated__ by Terraform
# Please review these resources and move them into your main configuration files.

# __generated__ by Terraform from "lux-discord-bot/FunctionUrlAllowPublicAccess-0d8663b9-3bd9-4972-accb-0626b64e8500"
resource "aws_lambda_permission" "function_url_public_access" {
  action                   = "lambda:InvokeFunctionUrl"
  function_name            = "lux-discord-bot"
  function_url_auth_type   = "NONE"
  principal                = "*"
  region                   = "us-west-1"
  statement_id             = "FunctionUrlAllowPublicAccess-0d8663b9-3bd9-4972-accb-0626b64e8500"
}

# __generated__ by Terraform from "lux-discord-bot"
resource "aws_lambda_function_url" "lux_discord_bot" {
  authorization_type = "NONE"
  function_name      = "lux-discord-bot"
  invoke_mode        = "BUFFERED"
  region             = "us-west-1"
}

# __generated__ by Terraform from "/aws/lambda/lux-discord-bot"
resource "aws_cloudwatch_log_group" "lux_discord_bot" {
  deletion_protection_enabled = false
  log_group_class             = "STANDARD"
  name                        = "/aws/lambda/lux-discord-bot"
  region                      = "us-west-1"
  retention_in_days           = 0
  skip_destroy                = false
  tags                        = {}
}

# __generated__ by Terraform
# code deployed by cargo-lambda; Terraform owns configuration only.
# filename is a never-read placeholder (ignore_changes) — required by schema.
resource "aws_lambda_function" "lux_discord_bot" {
  filename = "deployed-by-cargo-lambda.zip"
  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }
  architectures                      = ["x86_64"]
  function_name                      = "lux-discord-bot"
  handler                            = "bootstrap"
  layers                             = []
  memory_size                        = 128
  package_type                       = "Zip"
  region                             = "us-west-1"
  reserved_concurrent_executions     = -1
  role                               = "arn:aws:iam::735853783919:role/cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5"
  runtime                            = "provided.al2023"
  s3_bucket                          = null
  s3_key                             = null
  s3_object_version                  = null
  skip_destroy                       = false
  tags                               = {}
  timeout                            = 30
  ephemeral_storage {
    size = 512
  }
  logging_config {
    log_format            = "Text"
    log_group             = "/aws/lambda/lux-discord-bot"
  }
  tracing_config {
    mode = "PassThrough"
  }
}

# __generated__ by Terraform from "cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5"
resource "aws_iam_role" "cargo_lambda_role" {
  assume_role_policy = jsonencode({
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = {
        Service = "lambda.amazonaws.com"
      }
    }]
    Version = "2012-10-17"
  })
  force_detach_policies = false
  max_session_duration  = 3600
  name                  = "cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5"
  path                  = "/"
  tags                  = {}
}

# __generated__ by Terraform from "cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5/arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
resource "aws_iam_role_policy_attachment" "cargo_lambda_basic_execution" {
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
  role       = "cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5"
}
