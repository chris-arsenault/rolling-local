module "api" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/alb-api"

  prefix    = local.prefix
  hostname  = local.api
  zone_name = "ahara.io"
  vpc       = module.ctx.vpc
  alb       = module.ctx.alb

  environment = {
    TRUCK_TABLE             = aws_dynamodb_table.trucks.name
    MENU_BUCKET             = aws_s3_bucket.menus.id
    PUBLIC_SITE             = local.site_origin
    AUTH0_ISSUER            = local.auth0_issuer
    AUTH0_AUDIENCE          = auth0_resource_server.operator.identifier
    AUTH0_CONNECTION        = local.auth0_connection
    AUTH0_WEB_CLIENT_ID     = auth0_client.operator["web"].client_id
    AUTH0_IOS_CLIENT_ID     = auth0_client.operator["ios"].client_id
    AUTH0_ANDROID_CLIENT_ID = auth0_client.operator["android"].client_id
    MOBILE_REDIRECT_URI     = local.mobile_route
    RUST_LOG                = "info"
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
      routes = [
        {
          priority      = 475
          paths         = ["/health", "/public/*"]
          methods       = ["GET"]
          authenticated = false
        },
        {
          priority      = 474
          paths         = ["/operator", "/operator/*", "/admin/*"]
          methods       = ["OPTIONS"]
          authenticated = false
        },
      ]
    }
    operator = {
      binary      = "${path.module}/../../backend/target/lambda/rolling-local-api/bootstrap"
      environment = { SURFACE = "operator", ADMIN_SUBJECT = var.admin_subject }
      routes      = []
    }
  }
}

data "aws_lb_target_group" "operator" {
  name       = "${local.prefix}-operator-tg"
  depends_on = [module.api]
}

moved {
  from = module.api.aws_lb_listener_rule.this["operator-0"]
  to   = aws_lb_listener_rule.operator
}

resource "aws_lb_listener_rule" "operator" {
  listener_arn = module.ctx.alb.listener_arn
  priority     = 476

  condition {
    host_header {
      values = [local.api]
    }
  }

  condition {
    path_pattern {
      values = ["/operator", "/operator/*", "/admin/*"]
    }
  }
  action {
    type = "jwt-validation"

    jwt_validation {
      issuer        = local.auth0_issuer
      jwks_endpoint = "${local.auth0_issuer}.well-known/jwks.json"

      additional_claim {
        format = "string-array"
        name   = "aud"
        values = [auth0_resource_server.operator.identifier]
      }
    }
  }

  action {
    type             = "forward"
    target_group_arn = data.aws_lb_target_group.operator.arn
  }
}
