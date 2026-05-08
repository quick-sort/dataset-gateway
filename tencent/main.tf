terraform {
  required_version = ">= 1.0"
  required_providers {
    tencentcloud = {
      source  = "tencentcloudstack/tencentcloud"
      version = "~> 1.0"
    }
  }
}

provider "tencentcloud" {
  region = var.tencent_region
}

variable "tencent_region" {
  description = "Tencent Cloud region for deployment"
  type        = string
  default     = "ap-tokyo"
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
    "api_key_tc_123" = {
      bucket           = "example-bucket-12345"
      allowed_prefixes = ["userA/", "public/"]
    }
  }
}

locals {
  tags = {
    Project     = var.project_name
    Environment = "production"
    ManagedBy   = "opentofu"
  }
}
