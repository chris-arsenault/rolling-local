terraform {
  required_version = ">= 1.12"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.0"
    }
    auth0 = {
      source  = "auth0/auth0"
      version = "1.59.0"
    }
  }

  backend "s3" {
    region       = "us-east-1"
    key          = "projects/rolling-local.tfstate"
    encrypt      = true
    use_lockfile = true
  }
}

provider "aws" {
  region = "us-east-1"
  default_tags {
    tags = {
      Project   = "rolling-local"
      ManagedBy = "Terraform"
    }
  }
}

locals {
  prefix       = "rolling-local"
  site         = "rolling-local.ahara.io"
  api          = "api.rolling-local.ahara.io"
  mobile_route = "rollinglocal://oauth"
}

data "aws_caller_identity" "current" {}

module "ctx" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/platform-context"
}
