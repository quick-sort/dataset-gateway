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
  default = {}
}

variable "tags" {
  description = "Tags to apply to all resources"
  type        = map(string)
  default = {
    Project     = "dataset-gateway"
    Environment = "production"
    ManagedBy   = "opentofu"
  }
}
