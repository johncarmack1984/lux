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

# The plan reads lux/discord-public-key via a data source, so the plan role
# needs GetSecretValue on just that one (public-key) secret — ReadOnlyAccess
# deliberately excludes secret values. The apply role already covers it via its
# broader secret:lux/* grant.
resource "aws_iam_role_policy" "terraform_plan_discord_key" {
  name = "read-discord-public-key"
  role = aws_iam_role.terraform_plan.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue", "secretsmanager:DescribeSecret"]
      Resource = "arn:aws:secretsmanager:*:${local.aws_account_id}:secret:lux/discord-public-key*"
    }]
  })
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
          # Provider refresh reads code-signing config on every function refresh
          # (discovered on the first CI apply — plan never catches these because
          # the plan role runs on ReadOnlyAccess, which includes all reads).
          "lambda:GetFunctionCodeSigningConfig",
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
          # Provider refresh reads thing-principal attachments via the V2 API
          # (first-CI-apply discovery, like GetFunctionCodeSigningConfig above).
          "iot:ListThingPrincipalsV2",
          # The nudge channel's custom authorizer (nudge.tf).
          "iot:CreateAuthorizer", "iot:DescribeAuthorizer", "iot:UpdateAuthorizer", "iot:DeleteAuthorizer",
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
        Sid    = "ReadLuxSecrets"
        Effect = "Allow"
        Action = [
          "secretsmanager:GetSecretValue", "secretsmanager:DescribeSecret",
          # Provider refresh reads each secret's resource policy (first-CI-apply
          # discovery, like GetFunctionCodeSigningConfig above).
          "secretsmanager:GetResourcePolicy",
        ]
        Resource = "arn:aws:secretsmanager:*:${local.aws_account_id}:secret:lux/*"
      },
      {
        # The product site's bucket (site.tf). Reads are broad on our own
        # bucket because the provider refreshes every bucket sub-resource;
        # writes are the specific ones Terraform manages.
        Sid    = "ManageSiteBucket"
        Effect = "Allow"
        Action = [
          "s3:CreateBucket", "s3:DeleteBucket", "s3:Get*", "s3:List*",
          "s3:PutBucketPolicy", "s3:DeleteBucketPolicy",
          "s3:PutBucketPublicAccessBlock", "s3:PutBucketTagging",
          "s3:PutBucketOwnershipControls", "s3:PutEncryptionConfiguration",
          "s3:PutBucketVersioning", "s3:PutBucketAcl",
        ]
        Resource = [
          "arn:aws:s3:::lux-johncarmack-com",
          "arn:aws:s3:::lux-johncarmack-com/*",
        ]
      },
      {
        # CloudFront has no create-time resource ARNs to scope to.
        Sid    = "ManageSiteCloudFront"
        Effect = "Allow"
        Action = [
          "cloudfront:CreateDistribution", "cloudfront:CreateDistributionWithTags",
          "cloudfront:GetDistribution", "cloudfront:UpdateDistribution",
          "cloudfront:DeleteDistribution", "cloudfront:TagResource",
          "cloudfront:UntagResource", "cloudfront:ListTagsForResource",
          "cloudfront:CreateOriginAccessControl", "cloudfront:GetOriginAccessControl",
          "cloudfront:UpdateOriginAccessControl", "cloudfront:DeleteOriginAccessControl",
          "cloudfront:CreateFunction", "cloudfront:DescribeFunction",
          "cloudfront:GetFunction", "cloudfront:UpdateFunction",
          "cloudfront:DeleteFunction", "cloudfront:PublishFunction",
        ]
        Resource = "*"
      },
      {
        # ACM likewise: RequestCertificate has no a-priori ARN.
        Sid    = "ManageSiteCertificate"
        Effect = "Allow"
        Action = [
          "acm:RequestCertificate", "acm:DescribeCertificate",
          "acm:DeleteCertificate", "acm:ListTagsForCertificate",
          "acm:AddTagsToCertificate",
        ]
        Resource = "*"
      },
      {
        # Only the johncarmack.com zone; the site's A/AAAA + cert-validation
        # records live there (site.tf).
        Sid    = "ManageSiteDns"
        Effect = "Allow"
        Action = [
          "route53:GetHostedZone", "route53:ListResourceRecordSets",
          "route53:ChangeResourceRecordSets", "route53:ListTagsForResource",
        ]
        Resource = "arn:aws:route53:::hostedzone/Z2H7X9SMXZDQEI"
      },
      {
        Sid      = "ReadSiteDnsLookups"
        Effect   = "Allow"
        Action   = ["route53:ListHostedZones", "route53:GetChange"]
        Resource = "*"
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

# --- lambda-deploy role: ship Lambda code on release, main branch only -------
#
# Assumed by release.yml's deploy-lambdas job (after terraform-apply, before the
# desktop build), so deployed Lambda code always matches the release tag instead
# of drifting behind manual `cargo lambda deploy` runs. Deliberately narrow:
# update code + read config on the three lux functions, plus what the post-deploy
# smoke tests need (the Function URLs, and a direct test invoke of the
# authorizer's deny path). It cannot touch IAM, config, or any other resource.

locals {
  lux_lambda_functions = [
    "arn:aws:lambda:*:${local.aws_account_id}:function:lux-sync-api",
    "arn:aws:lambda:*:${local.aws_account_id}:function:lux-iot-authorizer",
    "arn:aws:lambda:*:${local.aws_account_id}:function:lux-discord-bot",
  ]
}

resource "aws_iam_role" "lambda_deploy" {
  name = "lux-lambda-deploy"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          # Only a workflow running on main (the release deploy) may assume this.
          "token.actions.githubusercontent.com:sub" = "${local.github_repo_sub}:ref:refs/heads/main"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "lambda_deploy" {
  name = "lux-lambda-deploy"
  role = aws_iam_role.lambda_deploy.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "DeployLuxLambdaCode"
        Effect = "Allow"
        Action = [
          "lambda:GetFunction", "lambda:GetFunctionConfiguration",
          "lambda:UpdateFunctionCode", "lambda:GetFunctionUrlConfig",
        ]
        Resource = local.lux_lambda_functions
      },
      {
        Sid      = "SmokeInvokeAuthorizer"
        Effect   = "Allow"
        Action   = "lambda:InvokeFunction"
        Resource = "arn:aws:lambda:*:${local.aws_account_id}:function:lux-iot-authorizer"
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

output "lambda_deploy_role_arn" {
  description = "role-to-assume for the release lambda-deploy job (main only)."
  value       = aws_iam_role.lambda_deploy.arn
}
