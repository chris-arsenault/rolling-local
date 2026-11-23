resource "aws_ssm_parameter" "application" {
  name = "/${local.prefix}/application"
  type = "String"
  value = jsonencode({
    truckTable      = aws_dynamodb_table.trucks.name
    extractionTable = aws_dynamodb_table.extractions.name
    menuBucket      = aws_s3_bucket.menus.id
    publicSite      = local.site_origin
    issuer          = local.auth0_issuer
    audience        = auth0_resource_server.operator.identifier
    connection      = local.auth0_connection
    webClientId     = auth0_client.operator["web"].client_id
    iosClientId     = auth0_client.operator["ios"].client_id
    androidClientId = auth0_client.operator["android"].client_id
    redirectUri     = local.mobile_route
  })
}

resource "aws_ssm_parameter" "admin_subject" {
  name  = "/${local.prefix}/admin-subject"
  type  = "String"
  value = data.auth0_user.owner.id
}

module "api" {
  source = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/alb-api"

  prefix    = local.prefix
  hostname  = local.api
  zone_name = "ahara.io"
  vpc       = module.ctx.vpc
  alb       = module.ctx.alb

  iam_policy = [jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = ["ssm:GetParameter"]
        Resource = [
          aws_ssm_parameter.application.arn,
          aws_ssm_parameter.admin_subject.arn,
        ]
      },
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
        Action   = ["dynamodb:GetItem", "dynamodb:PutItem"]
        Resource = aws_dynamodb_table.extractions.arn
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
      binary = "${path.module}/../../backend/target/lambda/rolling-local-api/bootstrap"
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
      binary = "${path.module}/../../backend/target/lambda/rolling-local-api/bootstrap"
      routes = []
    }
  }

  depends_on = [aws_ssm_parameter.application, aws_ssm_parameter.admin_subject]
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
