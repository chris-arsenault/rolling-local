data "aws_ssm_parameter" "auth0_domain" {
  name = "/ahara/auth0/domain"
}

data "aws_ssm_parameter" "auth0_client_id" {
  name = "/ahara/auth0/client-id"
}

data "aws_ssm_parameter" "auth0_client_secret" {
  name = "/ahara/auth0/client-secret"
}

data "aws_ssm_parameter" "auth0_issuer" {
  name = "/ahara/auth0/issuer"
}

data "aws_ssm_parameter" "auth0_connection_id" {
  name = "/ahara/auth0/connection-id"
}

data "aws_ssm_parameter" "auth0_connection_name" {
  name = "/ahara/auth0/connection-name"
}

provider "auth0" {
  domain        = nonsensitive(data.aws_ssm_parameter.auth0_domain.value)
  client_id     = nonsensitive(data.aws_ssm_parameter.auth0_client_id.value)
  client_secret = data.aws_ssm_parameter.auth0_client_secret.value
}

locals {
  site_origin      = "https://${local.site}"
  auth0_issuer     = nonsensitive(data.aws_ssm_parameter.auth0_issuer.value)
  auth0_audience   = "https://${local.api}"
  auth0_connection = nonsensitive(data.aws_ssm_parameter.auth0_connection_name.value)
  # Ahara's seed administrator, declared in ahara-infra services/variables.tf.
  owner_email = "chris@chris-arsenault.net"
  auth0_apps = {
    web     = { name = "Rolling Local", app_type = "spa", callbacks = ["${local.site_origin}/register", "${local.site_origin}/admin"] }
    ios     = { name = "Rolling Local for iOS", app_type = "native", callbacks = [local.mobile_route] }
    android = { name = "Rolling Local for Android", app_type = "native", callbacks = [local.mobile_route] }
  }
}

data "auth0_user" "owner" {
  query            = "email:${jsonencode(local.owner_email)} AND identities.connection:${jsonencode(local.auth0_connection)}"
  skip_permissions = true
  skip_roles       = true

  lifecycle {
    postcondition {
      condition = (
        self.email_verified && !self.blocked &&
        lower(self.email) == local.owner_email &&
        self.connection_name == local.auth0_connection
      )
      error_message = "Rolling Local requires the verified, enabled owner account in Ahara’s shared Auth0 connection."
    }
  }
}

resource "auth0_client" "operator" {
  for_each            = local.auth0_apps
  name                = each.value.name
  app_type            = each.value.app_type
  is_first_party      = true
  oidc_conformant     = true
  grant_types         = each.key == "web" ? ["authorization_code"] : ["authorization_code", "refresh_token"]
  callbacks           = each.value.callbacks
  allowed_logout_urls = each.key == "web" ? each.value.callbacks : []
  web_origins         = each.key == "web" ? [local.site_origin] : []
  allowed_origins     = each.key == "web" ? [local.site_origin] : []

  jwt_configuration {
    alg = "RS256"
  }

  dynamic "refresh_token" {
    for_each = each.key == "web" ? [] : [1]
    content {
      expiration_type = "expiring"
      rotation_type   = "rotating"
    }
  }
}

resource "auth0_client_credentials" "operator" {
  for_each              = local.auth0_apps
  client_id             = auth0_client.operator[each.key].client_id
  authentication_method = "none"
}

resource "auth0_connection_client" "operator" {
  for_each      = local.auth0_apps
  connection_id = nonsensitive(data.aws_ssm_parameter.auth0_connection_id.value)
  client_id     = auth0_client.operator[each.key].client_id
}

resource "auth0_resource_server" "operator" {
  identifier           = local.auth0_audience
  name                 = "Rolling Local API"
  signing_alg          = "RS256"
  allow_offline_access = true

  skip_consent_for_verifiable_first_party_clients = true
  subject_type_authorization {
    user {
      policy = "require_client_grant"
    }
    client {
      policy = "deny_all"
    }
  }
}

resource "auth0_client_grant" "operator" {
  for_each     = local.auth0_apps
  client_id    = auth0_client.operator[each.key].client_id
  audience     = auth0_resource_server.operator.identifier
  scopes       = []
  subject_type = "user"
}
