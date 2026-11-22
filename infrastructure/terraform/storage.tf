resource "aws_dynamodb_table" "trucks" {
  name         = "${local.prefix}-trucks"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "id"

  attribute {
    name = "id"
    type = "S"
  }

  ttl {
    attribute_name = "expires"
    enabled        = true
  }
}

resource "aws_s3_bucket" "menus" {
  bucket = "${local.prefix}-menus-${data.aws_caller_identity.current.account_id}"
}

resource "aws_s3_bucket_public_access_block" "menus" {
  bucket                  = aws_s3_bucket.menus.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_lifecycle_configuration" "uploads" {
  bucket = aws_s3_bucket.menus.id
  rule {
    id     = "expire-staged-uploads"
    status = "Enabled"
    filter {
      prefix = "uploads/"
    }
    expiration {
      days = 1
    }
  }
}
