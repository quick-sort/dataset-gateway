# Tencent Cloud API Gateway Configuration
# Note: The Tencent Cloud API Gateway Terraform provider has different structure
# than AWS API Gateway. For production, consider using the console or Tencent Cloud
# console-based API creation, then importing resources to Terraform.

resource "tencentcloud_api_gateway_service" "main" {
  service_name = "${var.project_name}-service"
  protocol     = "http"
  net_type     = ["OUTER"]
}

output "api_gateway_url" {
  value = tencentcloud_api_gateway_service.main.internal_sub_domain
}

output "api_gateway_service_id" {
  value = tencentcloud_api_gateway_service.main.id
}
