module "website" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/website"

  prefix         = local.prefix
  hostname       = local.site
  site_directory = "${path.module}/../../frontend/dist"
  encrypt        = false
  spa_fallback   = false

  runtime_config = {
    apiBaseUrl = "https://${local.api}"
  }

  data_origin = {
    domain_name = aws_s3_bucket.menus.bucket_regional_domain_name
  }
}

# The app has a small set of direct-entry pages. Serve its shell at those exact
# keys while keeping missing menu photos and other missing objects as errors.
resource "aws_s3_object" "page" {
  for_each      = toset(["about", "register", "admin"])
  bucket        = module.website.bucket_name
  key           = each.key
  source        = "${path.module}/../../frontend/dist/index.html"
  source_hash   = filemd5("${path.module}/../../frontend/dist/index.html")
  content_type  = "text/html; charset=utf-8"
  cache_control = "no-cache"
}

resource "aws_s3_bucket_policy" "menus" {
  bucket = aws_s3_bucket.menus.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid       = "ReadPublishedMenus"
      Effect    = "Allow"
      Principal = { Service = "cloudfront.amazonaws.com" }
      Action    = "s3:GetObject"
      Resource  = "${aws_s3_bucket.menus.arn}/data/menus/*"
      Condition = { StringEquals = { "AWS:SourceArn" = module.website.distribution_arn } }
    }]
  })
  depends_on = [aws_s3_bucket_public_access_block.menus]
}

output "website_url" {
  value = module.website.url
}

output "api_url" {
  value = "https://${module.api.hostname}"
}
