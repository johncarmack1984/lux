# CI-driven Terraform: the lux repo's GitHub Actions plan (on PRs) and apply (on
# a release-please version-bump merge) via OIDC — no local AWS creds, no local
# `terraform apply`. Two roles so PR runs can never mutate:
#   - lux-terraform-plan  : ReadOnlyAccess, assumable from any ref (PR plans).
#   - lux-terraform-apply : curated least-privilege write, main branch only.
# The roles are bootstrapped once with a local apply (chicken-and-egg: CI can't
# create the role it assumes); thereafter all changes flow through CI. Reuses the
# GitHub OIDC provider data source from release-signing.tf.

locals {
  aws_account_id  = "735853783919"
  tf_state_bucket = "john-carmack-terraform-state"
  tf_state_prefix = "lux/"
  github_repo_sub = "repo:johncarmack1984/lux"
  managed_role_arns = [
    "arn:aws:iam::735853783919:role/lux*",
    "arn:aws:iam::735853783919:role/cargo-lambda-role-*",
  ]
}

# --- plan role: read-only, any ref (PR plans run with -lock=false) -----------

resource "aws_iam_role" "terraform_plan" {
  name = "lux-terraform-plan"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
        StringLike   = { "token.actions.githubusercontent.com:sub" = "${local.github_repo_sub}:*" }
      }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "terraform_plan_readonly" {
  role       = aws_iam_role.terraform_plan.name
  policy_arn = "arn:aws:iam::aws:policy/ReadOnlyAccess"
}

# --- apply role: curated write, main branch only -----------------------------

resource "aws_iam_role" "terraform_apply" {
  name = "lux-terraform-apply"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          # Only a workflow running on main (the release apply) may assume this.
          "token.actions.githubusercontent.com:sub" = "${local.github_repo_sub}:ref:refs/heads/main"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "terraform_apply" {
  name = "lux-terraform-apply"
  role = aws_iam_role.terraform_apply.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "TerraformStateBackend"
        Effect   = "Allow"
        Action   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"]
        Resource = "arn:aws:s3:::${local.tf_state_bucket}/${local.tf_state_prefix}*"
      },
      {
        Sid       = "TerraformStateList"
        Effect    = "Allow"
        Action    = "s3:ListBucket"
        Resource  = "arn:aws:s3:::${local.tf_state_bucket}"
        Condition = { StringLike = { "s3:prefix" = ["${local.tf_state_prefix}*"] } }
      },
      {
        Sid    = "ManageLuxIamRoles"
        Effect = "Allow"
        Action = [
          "iam:CreateRole", "iam:GetRole", "iam:DeleteRole", "iam:UpdateRole",
          "iam:UpdateAssumeRolePolicy", "iam:TagRole", "iam:UntagRole", "iam:ListRoleTags",
          "iam:PutRolePolicy", "iam:GetRolePolicy", "iam:DeleteRolePolicy",
          "iam:ListRolePolicies", "iam:ListAttachedRolePolicies",
          "iam:AttachRolePolicy", "iam:DetachRolePolicy", "iam:ListInstanceProfilesForRole",
        ]
        Resource = local.managed_role_arns
      },
      {
        Sid       = "PassLuxLambdaRoles"
        Effect    = "Allow"
        Action    = "iam:PassRole"
        Resource  = local.managed_role_arns
        Condition = { StringEquals = { "iam:PassedToService" = "lambda.amazonaws.com" } }
      },
      {
        Sid      = "ReadIamForDataSourcesAndAttachments"
        Effect   = "Allow"
        Action   = ["iam:GetOpenIDConnectProvider", "iam:ListOpenIDConnectProviders", "iam:GetPolicy", "iam:GetPolicyVersion"]
        Resource = "*"
      },
      {
        Sid    = "ManageLuxLambda"
        Effect = "Allow"
        Action = [
          "lambda:CreateFunction", "lambda:GetFunction", "lambda:GetFunctionConfiguration",
          "lambda:UpdateFunctionCode", "lambda:UpdateFunctionConfiguration", "lambda:DeleteFunction",
          "lambda:ListVersionsByFunction", "lambda:AddPermission", "lambda:RemovePermission",
          "lambda:GetPolicy", "lambda:CreateFunctionUrlConfig", "lambda:GetFunctionUrlConfig",
          "lambda:UpdateFunctionUrlConfig", "lambda:DeleteFunctionUrlConfig",
          "lambda:TagResource", "lambda:UntagResource", "lambda:ListTags",
        ]
        Resource = "arn:aws:lambda:*:${local.aws_account_id}:function:lux*"
      },
      {
        Sid    = "ManageLuxLogGroups"
        Effect = "Allow"
        Action = [
          "logs:CreateLogGroup", "logs:DeleteLogGroup", "logs:PutRetentionPolicy",
          "logs:TagLogGroup", "logs:UntagLogGroup", "logs:ListTagsForResource",
          "logs:ListTagsLogGroup", "logs:TagResource", "logs:UntagResource",
        ]
        Resource = [
          "arn:aws:logs:*:${local.aws_account_id}:log-group:/aws/lambda/lux*",
          "arn:aws:logs:*:${local.aws_account_id}:log-group:/aws/lambda/lux*:*",
        ]
      },
      {
        Sid      = "DescribeLogGroups"
        Effect   = "Allow"
        Action   = "logs:DescribeLogGroups"
        Resource = "*"
      },
      {
        Sid    = "ManageLuxIot"
        Effect = "Allow"
        Action = [
          "iot:CreateThing", "iot:DescribeThing", "iot:DeleteThing", "iot:ListThingPrincipals",
          "iot:CreateKeysAndCertificate", "iot:DescribeCertificate", "iot:UpdateCertificate",
          "iot:DeleteCertificate", "iot:CreatePolicy", "iot:GetPolicy", "iot:DeletePolicy",
          "iot:ListPolicyVersions", "iot:ListTargetsForPolicy", "iot:AttachPolicy", "iot:DetachPolicy",
          "iot:ListAttachedPolicies", "iot:AttachThingPrincipal", "iot:DetachThingPrincipal",
          "iot:TagResource", "iot:UntagResource", "iot:ListTagsForResource",
        ]
        Resource = "arn:aws:iot:*:${local.aws_account_id}:*"
      },
      {
        Sid      = "IotDescribeEndpoint"
        Effect   = "Allow"
        Action   = "iot:DescribeEndpoint"
        Resource = "*"
      },
      {
        Sid    = "ManageLuxDynamoDb"
        Effect = "Allow"
        Action = [
          "dynamodb:CreateTable", "dynamodb:DescribeTable", "dynamodb:DeleteTable", "dynamodb:UpdateTable",
          "dynamodb:DescribeContinuousBackups", "dynamodb:UpdateContinuousBackups",
          "dynamodb:DescribeTimeToLive", "dynamodb:UpdateTimeToLive",
          "dynamodb:ListTagsOfResource", "dynamodb:TagResource", "dynamodb:UntagResource",
        ]
        Resource = "arn:aws:dynamodb:*:${local.aws_account_id}:table/lux-sync"
      },
      {
        Sid    = "ManageLuxCognito"
        Effect = "Allow"
        Action = [
          "cognito-idp:CreateUserPool", "cognito-idp:DescribeUserPool", "cognito-idp:UpdateUserPool",
          "cognito-idp:DeleteUserPool", "cognito-idp:CreateUserPoolClient", "cognito-idp:DescribeUserPoolClient",
          "cognito-idp:UpdateUserPoolClient", "cognito-idp:DeleteUserPoolClient",
          "cognito-idp:GetUserPoolMfaConfig", "cognito-idp:SetUserPoolMfaConfig",
          "cognito-idp:TagResource", "cognito-idp:UntagResource", "cognito-idp:ListTagsForResource",
        ]
        # CreateUserPool has no resource at create time; the pool id isn't known
        # a priori, so this is scoped to the account/region's user pools.
        Resource = "*"
      },
      {
        Sid      = "ReadLuxSecrets"
        Effect   = "Allow"
        Action   = ["secretsmanager:GetSecretValue", "secretsmanager:DescribeSecret"]
        Resource = "arn:aws:secretsmanager:*:${local.aws_account_id}:secret:lux/*"
      },
      {
        Sid      = "CallerIdentity"
        Effect   = "Allow"
        Action   = "sts:GetCallerIdentity"
        Resource = "*"
      },
    ]
  })
}

output "terraform_plan_role_arn" {
  description = "role-to-assume for the terraform PR plan job (read-only)."
  value       = aws_iam_role.terraform_plan.arn
}

output "terraform_apply_role_arn" {
  description = "role-to-assume for the terraform apply job (main only)."
  value       = aws_iam_role.terraform_apply.arn
}
