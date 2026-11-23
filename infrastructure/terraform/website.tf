module "website" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/website"

  prefix         = local.prefix
  hostname       = local.site
  site_directory = "${path.module}/../../frontend/dist"
  encrypt        = false
  vpc            = module.ctx.vpc
  og_artifact    = module.ctx.og_server

  og_config = {
    site_name = "Rolling Local"
    defaults = {
      title       = "Find food trucks"
      description = "Find nearby food trucks, see who's sharing a location, and browse their menus."
      image       = "/og-default.png"
    }
    routes = [
      {
        pattern     = "/about"
        title       = "Rolling Local for truck operators"
        description = "Show customers where you're serving, share your menu, and stay discoverable between stops."
        og_type     = "website"
      },
      {
        pattern     = "/register"
        title       = "Register your truck"
        description = "Submit your food truck for review and check your registration status."
        og_type     = "website"
      },
      {
        pattern     = "/admin"
        title       = "Truck administration"
        description = "Sign in to manage Rolling Local truck listings."
        og_type     = "website"
      },
    ]
  }

  runtime_config = {
    apiBaseUrl = "https://${local.api}"
  }

  data_origin = {
    domain_name = aws_s3_bucket.menus.bucket_regional_domain_name
  }
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
