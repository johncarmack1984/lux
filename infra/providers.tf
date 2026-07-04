terraform {
  required_version = ">= 1.10"

  backend "s3" {
    bucket       = "john-carmack-terraform-state"
    key          = "lux/terraform.tfstate"
    region       = "us-west-2"
    use_lockfile = true
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.0"
    }
    # Builds the placeholder zip that lets `apply` create the sync-api Lambda
    # before cargo-lambda ships the real code (see accounts.tf).
    archive = {
      source  = "hashicorp/archive"
      version = "~> 2.0"
    }
  }
}

provider "aws" {
  region = "us-west-1"
}

# CloudFront only accepts ACM certificates issued in us-east-1 (site.tf).
provider "aws" {
  alias  = "us_east_1"
  region = "us-east-1"
}
