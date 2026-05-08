resource "tencentcloud_cos_bucket" "dataset" {
  bucket = "${var.project_name}-dataset-${var.tencent_region}"

  tags = local.tags
}
