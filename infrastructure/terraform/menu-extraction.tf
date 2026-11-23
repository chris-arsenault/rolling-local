resource "aws_dynamodb_table" "extractions" {
  name             = "${local.prefix}-menu-items"
  billing_mode     = "PAY_PER_REQUEST"
  hash_key         = "truckId"
  range_key        = "id"
  stream_enabled   = true
  stream_view_type = "NEW_IMAGE"

  attribute {
    name = "id"
    type = "S"
  }
  attribute {
    name = "truckId"
    type = "S"
  }
}

resource "aws_sqs_queue" "extraction_failures" {
  name                      = "${local.prefix}-menu-failures"
  message_retention_seconds = 1209600
  sqs_managed_sse_enabled   = true
}

resource "aws_iam_role" "menu_worker" {
  name = "${local.prefix}-menu-worker"
  assume_role_policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Principal = { Service = "lambda.amazonaws.com" }, Action = "sts:AssumeRole" }]
  })
}

resource "aws_iam_role_policy_attachment" "menu_worker_vpc" {
  role       = aws_iam_role.menu_worker.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaVPCAccessExecutionRole"
}

locals {
  menu_model       = "openai.gpt-6.2-sol"
  menu_profile     = "us.${local.menu_model}"
  menu_profile_arn = "arn:aws:bedrock:${local.region}:${data.aws_caller_identity.current.account_id}:inference-profile/${local.menu_profile}"
}

resource "aws_iam_role_policy" "menu_worker" {
  name = "${local.prefix}-menu-extraction"
  role = aws_iam_role.menu_worker.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      { Effect = "Allow", Action = "dynamodb:GetItem", Resource = aws_dynamodb_table.trucks.arn },
      { Effect = "Allow", Action = ["dynamodb:GetItem", "dynamodb:PutItem"], Resource = aws_dynamodb_table.extractions.arn },
      { Effect = "Allow", Action = ["dynamodb:DescribeStream", "dynamodb:GetRecords", "dynamodb:GetShardIterator"], Resource = aws_dynamodb_table.extractions.stream_arn },
      { Effect = "Allow", Action = "dynamodb:ListStreams", Resource = "*" },
      { Effect = "Allow", Action = "sqs:SendMessage", Resource = aws_sqs_queue.extraction_failures.arn },
      { Effect = "Allow", Action = "s3:GetObject", Resource = "${aws_s3_bucket.menus.arn}/data/menus/*" },
      { Effect = "Allow", Action = "bedrock:InvokeModel", Resource = local.menu_profile_arn },
      {
        Effect   = "Allow"
        Action   = "bedrock:InvokeModel"
        Resource = [for region in ["us-east-1", "us-east-2", "us-west-2"] : "arn:aws:bedrock:${region}::foundation-model/${local.menu_model}"]
        Condition = {
          StringEquals = { "bedrock:InferenceProfileArn" = local.menu_profile_arn }
        }
      },
    ]
  })
}

module "menu_worker" {
  source                         = "git::https://github.com/chris-arsenault/ahara-tf-patterns.git//modules/lambda"
  name                           = "${local.prefix}-menu-worker"
  binary                         = "${path.module}/../../backend/target/lambda/menu-worker/bootstrap"
  vpc                            = module.ctx.vpc
  role_arn                       = aws_iam_role.menu_worker.arn
  timeout                        = 100
  reserved_concurrent_executions = 2
  environment = {
    TRUCK_TABLE      = aws_dynamodb_table.trucks.name
    EXTRACTION_TABLE = aws_dynamodb_table.extractions.name
    MENU_BUCKET      = aws_s3_bucket.menus.id
    BEDROCK_MODEL_ID = local.menu_profile
  }
  depends_on = [aws_iam_role_policy_attachment.menu_worker_vpc, aws_iam_role_policy.menu_worker]
}

resource "aws_lambda_event_source_mapping" "extract_new_menus" {
  function_name                 = module.menu_worker.function_name
  event_source_arn              = aws_dynamodb_table.extractions.stream_arn
  starting_position             = "TRIM_HORIZON"
  batch_size                    = 1
  maximum_retry_attempts        = 3
  maximum_record_age_in_seconds = 86400
  filter_criteria {
    filter { pattern = jsonencode({ dynamodb = { NewImage = { dispatch = { S = [{ exists = true }] } } } }) }
  }
  destination_config {
    on_failure { destination_arn = aws_sqs_queue.extraction_failures.arn }
  }
}
