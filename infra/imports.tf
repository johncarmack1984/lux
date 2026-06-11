import {
  to = aws_lambda_function.lux_discord_bot
  id = "lux-discord-bot"
}

import {
  to = aws_iam_role.cargo_lambda_role
  id = "cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5"
}

import {
  to = aws_iam_role_policy_attachment.cargo_lambda_basic_execution
  id = "cargo-lambda-role-4a5d5ad3-d07e-45cb-9528-4d1e34e454a5/arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

import {
  to = aws_lambda_function_url.lux_discord_bot
  id = "lux-discord-bot"
}

import {
  to = aws_cloudwatch_log_group.lux_discord_bot
  id = "/aws/lambda/lux-discord-bot"
}

import {
  to = aws_lambda_permission.function_url_public_access
  id = "lux-discord-bot/FunctionUrlAllowPublicAccess-0d8663b9-3bd9-4972-accb-0626b64e8500"
}
