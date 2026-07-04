# lux.johncarmack.com — the product site (apps/site, static build).
#
# S3 (private, OAC-only) behind CloudFront, ACM cert in us-east-1 (CloudFront's
# required region), and the DNS records in the johncarmack.com zone — the zone
# lives in this same account, so validation + alias records apply in one pass.
# Files ship via .github/workflows/site.yml (s3 sync + invalidation) under the
# lux-site-deploy role below; Terraform owns everything but the objects.

locals {
  site_domain = "lux.johncarmack.com"
  # No dots: a dotted bucket name breaks TLS between CloudFront and the S3
  # regional endpoint (multi-label hostname under a single-label wildcard cert).
  site_bucket = "lux-johncarmack-com"
}

data "aws_route53_zone" "johncarmack_com" {
  name         = "johncarmack.com."
  private_zone = false
}

# --- bucket (private; CloudFront is the only reader) --------------------------

resource "aws_s3_bucket" "site" {
  bucket = local.site_bucket
}

resource "aws_s3_bucket_public_access_block" "site" {
  bucket                  = aws_s3_bucket.site.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_policy" "site" {
  bucket = aws_s3_bucket.site.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid       = "CloudFrontOacRead"
      Effect    = "Allow"
      Principal = { Service = "cloudfront.amazonaws.com" }
      Action    = "s3:GetObject"
      Resource  = "${aws_s3_bucket.site.arn}/*"
      Condition = { StringEquals = { "AWS:SourceArn" = aws_cloudfront_distribution.site.arn } }
    }]
  })
}

# --- certificate (us-east-1) + DNS validation, one apply ----------------------

resource "aws_acm_certificate" "site" {
  provider          = aws.us_east_1
  domain_name       = local.site_domain
  validation_method = "DNS"

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_route53_record" "site_cert_validation" {
  for_each = {
    for dvo in aws_acm_certificate.site.domain_validation_options : dvo.domain_name => {
      name  = dvo.resource_record_name
      type  = dvo.resource_record_type
      value = dvo.resource_record_value
    }
  }

  zone_id = data.aws_route53_zone.johncarmack_com.zone_id
  name    = each.value.name
  type    = each.value.type
  ttl     = 300
  records = [each.value.value]
}

resource "aws_acm_certificate_validation" "site" {
  provider                = aws.us_east_1
  certificate_arn         = aws_acm_certificate.site.arn
  validation_record_fqdns = [for r in aws_route53_record.site_cert_validation : r.fqdn]
}

# --- CloudFront ----------------------------------------------------------------

resource "aws_cloudfront_origin_access_control" "site" {
  name                              = "lux-site"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

# S3 REST origins don't serve directory indexes; rewrite `/privacy/` (and any
# extensionless path) to its index.html at the edge.
resource "aws_cloudfront_function" "site_dir_index" {
  name    = "lux-site-dir-index"
  runtime = "cloudfront-js-2.0"
  publish = true
  code    = <<-EOT
    function handler(event) {
      var request = event.request;
      var uri = request.uri;
      if (uri.endsWith('/')) {
        request.uri = uri + 'index.html';
      } else if (!uri.includes('.')) {
        request.uri = uri + '/index.html';
      }
      return request;
    }
  EOT
}

resource "aws_cloudfront_distribution" "site" {
  enabled             = true
  is_ipv6_enabled     = true
  comment             = local.site_domain
  default_root_object = "index.html"
  aliases             = [local.site_domain]
  price_class         = "PriceClass_100"
  http_version        = "http2and3"

  origin {
    domain_name              = aws_s3_bucket.site.bucket_regional_domain_name
    origin_id                = "s3-site"
    origin_access_control_id = aws_cloudfront_origin_access_control.site.id
  }

  default_cache_behavior {
    target_origin_id       = "s3-site"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true
    # AWS managed "CachingOptimized" policy.
    cache_policy_id = "658327ea-f89d-4fab-a63d-7e88639e58f6"

    function_association {
      event_type   = "viewer-request"
      function_arn = aws_cloudfront_function.site_dir_index.arn
    }
  }

  # A missing key surfaces as S3 403 through OAC; serve the home document with
  # a 404 status rather than a bare XML error.
  custom_error_response {
    error_code         = 403
    response_code      = 404
    response_page_path = "/index.html"
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    acm_certificate_arn      = aws_acm_certificate_validation.site.certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }
}

# --- DNS -------------------------------------------------------------------------

resource "aws_route53_record" "site_a" {
  zone_id = data.aws_route53_zone.johncarmack_com.zone_id
  name    = local.site_domain
  type    = "A"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_route53_record" "site_aaaa" {
  zone_id = data.aws_route53_zone.johncarmack_com.zone_id
  name    = local.site_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

# --- deploy role: sync files + invalidate, main branch only ----------------------

resource "aws_iam_role" "site_deploy" {
  name = "lux-site-deploy"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          "token.actions.githubusercontent.com:sub" = "${local.github_repo_sub}:ref:refs/heads/main"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "site_deploy" {
  name = "lux-site-deploy"
  role = aws_iam_role.site_deploy.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "SyncSiteObjects"
        Effect   = "Allow"
        Action   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"]
        Resource = "${aws_s3_bucket.site.arn}/*"
      },
      {
        Sid      = "ListSiteBucket"
        Effect   = "Allow"
        Action   = "s3:ListBucket"
        Resource = aws_s3_bucket.site.arn
      },
      {
        # The workflow resolves the distribution by alias at run time (state,
        # not memory — no baked distribution id to go stale).
        Sid      = "FindDistribution"
        Effect   = "Allow"
        Action   = "cloudfront:ListDistributions"
        Resource = "*"
      },
      {
        Sid      = "InvalidateSite"
        Effect   = "Allow"
        Action   = ["cloudfront:CreateInvalidation", "cloudfront:GetInvalidation"]
        Resource = aws_cloudfront_distribution.site.arn
      },
    ]
  })
}

output "site_bucket" {
  description = "S3 bucket holding the built site (synced by site.yml)."
  value       = aws_s3_bucket.site.id
}

output "site_distribution_domain" {
  description = "CloudFront domain for lux.johncarmack.com."
  value       = aws_cloudfront_distribution.site.domain_name
}

output "site_deploy_role_arn" {
  description = "role-to-assume for the site deploy workflow (main only)."
  value       = aws_iam_role.site_deploy.arn
}
