# Store-notes drafting: the release PR gets a Claude-drafted App Store
# "What's New" file (workflow: store-notes.yml), which a human edits in the PR
# before appstore.yml will submit it. The Anthropic API key lives only in
# Secrets Manager (created out-of-band, like the other lux/* secrets).
#
# The role is deliberately separate from lux-release-signing: that role's
# trust is main-ref-only because it reads the updater signing key, and no PR
# workflow may ever assume it. Drafting runs on pull_request events (the
# release PR), so it gets its own role scoped to this one secret.

data "aws_secretsmanager_secret" "anthropic_api_key" {
  name = "lux/anthropic-api-key"
}

resource "aws_iam_role" "store_notes" {
  name = "lux-store-notes"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          # pull_request-context workflows in this repo only. Same-repo PRs
          # are maintainer-authored; fork PRs never receive OIDC here. The
          # blast radius of this role is exactly one spend-limited API key.
          "token.actions.githubusercontent.com:sub" = "repo:johncarmack1984/lux:pull_request"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "read_anthropic_api_key" {
  name = "read-anthropic-api-key"
  role = aws_iam_role.store_notes.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = "secretsmanager:GetSecretValue"
      Resource = [data.aws_secretsmanager_secret.anthropic_api_key.arn]
    }]
  })
}

output "store_notes_role_arn" {
  description = "Set as role-to-assume in the store-notes workflow (AWS OIDC)."
  value       = aws_iam_role.store_notes.arn
}
