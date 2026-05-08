terraform {
  required_version = ">= 1.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

variable "aws_region" {
  description = "AWS region for deployment"
  type        = string
  default     = "us-east-1"
}

variable "project_name" {
  description = "Project name for resource naming"
  type        = string
  default     = "dataset-gateway"
}

variable "api_key_permissions" {
  description = "API Key to bucket/prefix mappings"
  type        = map(object({
    bucket           = string
    allowed_prefixes = list(string)
  }))
  default = {
    "api_key_abc123" = {
      bucket           = "example-bucket"
      allowed_prefixes = ["userA/", "public/"]
    }
  }
}

data "aws_caller_identity" "current" {}

output "aws_account_id" {
  value = data.aws_caller_identity.current.account_id
}

locals {
  tags = {
    Project     = var.project_name
    Environment = "production"
    ManagedBy   = "opentofu"
  }
}
