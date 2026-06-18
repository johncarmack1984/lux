# Lets the lux repo's GitHub Actions read the Tauri updater signing key from
# Secrets Manager at release time via OIDC — no long-lived AWS keys in GitHub.
# The key itself lives only in AWS Secrets Manager (created out-of-band).

data "aws_iam_openid_connect_provider" "github" {
  url = "https://token.actions.githubusercontent.com"
}

data "aws_secretsmanager_secret" "updater_signing_key" {
  name = "lux/updater-signing-key"
}

data "aws_secretsmanager_secret" "apple_signing" {
  name = "lux/apple-signing"
}

resource "aws_iam_role" "release_signing" {
  name = "lux-release-signing"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
        }
        # Only the lux repo's workflows may assume this role.
        StringLike = {
          "token.actions.githubusercontent.com:sub" = "repo:johncarmack1984/lux:*"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "read_updater_signing_key" {
  name = "read-updater-signing-key"
  role = aws_iam_role.release_signing.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = "secretsmanager:GetSecretValue"
      Resource = [
        data.aws_secretsmanager_secret.updater_signing_key.arn,
        data.aws_secretsmanager_secret.apple_signing.arn,
      ]
    }]
  })
}

output "release_signing_role_arn" {
  description = "Set as role-to-assume in the release workflow (AWS OIDC)."
  value       = aws_iam_role.release_signing.arn
}
