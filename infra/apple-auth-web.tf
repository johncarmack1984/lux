# auth.lux.johncarmack.com — the verified domain the web (browser) Sign in with
# Apple flow needs (.claude/specs/sign-in-with-apple-web.md).
#
# Apple requires the Services ID's Return URL to sit on a domain you own and
# verify; a raw *.lambda-url.on.aws host can't be verified, so this fronts the
# lux-apple-auth Function URL with a CloudFront distribution on a custom domain
# (the same cert→CloudFront→Route53 shape as the site in site.tf). Only two
# paths are ever hit through here — Apple's `POST /auth/apple/web/callback` and
# the `GET /.well-known/apple-developer-domain-association.txt` verification
# file (served by the handler from a committed token). The app itself calls
# `/web/start` and `/web/exchange` straight on the Function URL (apple_auth_url);
# they never need the custom domain.

locals {
  apple_auth_domain = "auth.lux.johncarmack.com"
  # The Function URL host, sans scheme/trailing slash — CloudFront's origin.
  apple_auth_fn_host = trimsuffix(
    trimprefix(aws_lambda_function_url.lux_apple_auth.function_url, "https://"),
    "/",
  )
}

# --- certificate (us-east-1, CloudFront's required region) + DNS validation ---

resource "aws_acm_certificate" "apple_auth" {
  provider          = aws.us_east_1
  domain_name       = local.apple_auth_domain
  validation_method = "DNS"

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_route53_record" "apple_auth_cert_validation" {
  for_each = {
    for dvo in aws_acm_certificate.apple_auth.domain_validation_options : dvo.domain_name => {
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

resource "aws_acm_certificate_validation" "apple_auth" {
  provider                = aws.us_east_1
  certificate_arn         = aws_acm_certificate.apple_auth.arn
  validation_record_fqdns = [for r in aws_route53_record.apple_auth_cert_validation : r.fqdn]
}

# --- CloudFront: pass everything through to the Function URL -------------------

resource "aws_cloudfront_distribution" "apple_auth" {
  enabled         = true
  is_ipv6_enabled = true
  comment         = local.apple_auth_domain
  aliases         = [local.apple_auth_domain]
  price_class     = "PriceClass_100"
  http_version    = "http2and3"

  origin {
    domain_name = local.apple_auth_fn_host
    origin_id   = "lux-apple-auth-fn"

    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "https-only"
      origin_ssl_protocols   = ["TLSv1.2"]
    }
  }

  default_cache_behavior {
    target_origin_id       = "lux-apple-auth-fn"
    viewer_protocol_policy = "https-only"
    # Apple form_posts the callback; the app posts start/exchange.
    allowed_methods = ["GET", "HEAD", "OPTIONS", "PUT", "POST", "PATCH", "DELETE"]
    cached_methods  = ["GET", "HEAD"]
    compress        = true
    # Auth traffic is never cacheable — AWS managed "CachingDisabled" …
    cache_policy_id = "4135ea2d-6df8-44a3-9df3-4b5a84be39ad"
    # … and forward everything except Host, which a Function URL rejects if it
    # isn't its own (AWS managed "AllViewerExceptHostHeader").
    origin_request_policy_id = "b689b0a8-53d0-40ab-baf2-68738e2966ac"
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    acm_certificate_arn      = aws_acm_certificate_validation.apple_auth.certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }
}

# --- DNS ----------------------------------------------------------------------

resource "aws_route53_record" "apple_auth_a" {
  zone_id = data.aws_route53_zone.johncarmack_com.zone_id
  name    = local.apple_auth_domain
  type    = "A"

  alias {
    name                   = aws_cloudfront_distribution.apple_auth.domain_name
    zone_id                = aws_cloudfront_distribution.apple_auth.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_route53_record" "apple_auth_aaaa" {
  zone_id = data.aws_route53_zone.johncarmack_com.zone_id
  name    = local.apple_auth_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.apple_auth.domain_name
    zone_id                = aws_cloudfront_distribution.apple_auth.hosted_zone_id
    evaluate_target_health = false
  }
}

# Consumed by scripts/gen-endpoints in the follow-up endpoints unit: the app
# lights its web "Sign in with Apple" button when this is present (the raw
# presence is the signal — the Services ID + callback stay server-side). Absent
# field ⇒ feature dark, so adding the output here doesn't touch the drift gate.
output "apple_web_enabled" {
  description = "APPLE_WEB_ENABLED — web Sign in with Apple is provisioned (the .dmg/dev fallback)."
  value       = true
}
