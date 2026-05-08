resource "tencentcloud_scf_function" "auth_redirect" {
  name      = "${var.project_name}-auth-redirect"
  namespace = "default"
  runtime   = "Python3.11"
  handler   = "scf_function.main_handler"
  timeout   = 30

  tags = local.tags
}
