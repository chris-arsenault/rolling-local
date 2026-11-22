module "cognito" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/cognito-app"

  name                = "${local.prefix}-operator"
  cognito             = module.ctx.cognito
  callback_urls       = [local.mobile_route]
  public_oauth_client = true
}

resource "aws_ssm_parameter" "auth_trigger_client" {
  name  = "/ahara/auth-trigger/clients/rolling-local"
  type  = "String"
  value = module.cognito.client_id
}

module "api" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/alb-api"

  prefix    = local.prefix
  hostname  = local.api
  zone_name = "ahara.io"
  vpc       = module.ctx.vpc
  alb       = module.ctx.alb
  cognito   = module.ctx.cognito

  environment = {
    TRUCK_TABLE         = aws_dynamodb_table.trucks.name
    MENU_BUCKET         = aws_s3_bucket.menus.id
    PUBLIC_SITE         = "https://${local.site}"
    COGNITO_DOMAIN      = "https://${trimprefix(module.ctx.cognito.domain, "https://")}"
    COGNITO_CLIENT_ID   = module.cognito.client_id
    MOBILE_REDIRECT_URI = local.mobile_route
    RUST_LOG            = "info"
  }

  iam_policy = [jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "dynamodb:GetItem", "dynamodb:PutItem", "dynamodb:DeleteItem",
          "dynamodb:ConditionCheckItem", "dynamodb:Scan",
        ]
        Resource = aws_dynamodb_table.trucks.arn
      },
      {
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = aws_s3_bucket.menus.arn
      },
      {
        Effect   = "Allow"
        Action   = ["s3:GetObject", "s3:PutObject"]
        Resource = "${aws_s3_bucket.menus.arn}/uploads/*"
      },
      {
        Effect   = "Allow"
        Action   = ["s3:PutObject"]
        Resource = "${aws_s3_bucket.menus.arn}/data/menus/*"
      },
    ]
  })]

  lambdas = {
    public = {
      binary      = "${path.module}/../../backend/target/lambda/rolling-local-api/bootstrap"
      environment = { SURFACE = "public" }
      routes = [{
        priority      = 475
        paths         = ["/health", "/public/*"]
        methods       = ["GET"]
        authenticated = false
      }]
    }
    operator = {
      binary      = "${path.module}/../../backend/target/lambda/rolling-local-api/bootstrap"
      environment = { SURFACE = "operator" }
      routes = [{
        priority      = 476
        paths         = ["/operator", "/operator/*"]
        authenticated = true
      }]
    }
  }
}
