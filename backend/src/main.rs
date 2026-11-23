use chrono::Utc;
use lambda_http::{Error, Request, RequestExt, service_fn};
use rolling_local_api::{Api, OperatorConfig, Surface, media::S3Media, storage::DynamoStore};
use serde::Deserialize;
use std::sync::Arc;

const APPLICATION: &str = "/rolling-local/application";
const OWNER: &str = "/rolling-local/admin-subject";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Deployment {
    truck_table: String,
    extraction_table: String,
    menu_bucket: String,
    #[serde(flatten)]
    identity: OperatorConfig,
}

async fn deployment(ssm: &aws_sdk_ssm::Client) -> Result<Deployment, Error> {
    let result = ssm.get_parameter().name(APPLICATION).send().await?;
    let value = result
        .parameter()
        .and_then(|parameter| parameter.value())
        .ok_or("Missing Rolling Local application settings")?;
    Ok(serde_json::from_str(value)?)
}

async fn admin_subject(ssm: &aws_sdk_ssm::Client) -> Result<Option<String>, Error> {
    match ssm.get_parameter().name(OWNER).send().await {
        Ok(result) => Ok(result
            .parameter()
            .and_then(|parameter| parameter.value())
            .map(str::to_owned)),
        Err(error)
            if error
                .as_service_error()
                .is_some_and(|error| error.is_parameter_not_found()) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn invoked_surface(arn: &str) -> Result<Surface, Error> {
    // Use the Lambda invocation identity to keep public requests on the public surface.
    match arn.split(':').nth(6) {
        Some("rolling-local-public") => Ok(Surface::Public),
        Some("rolling-local-operator") => Ok(Surface::Operator),
        _ => Err("Unexpected Rolling Local Lambda identity".into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "Could not install the TLS crypto provider")?;
    tracing_subscriber::fmt().json().init();
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ssm = aws_sdk_ssm::Client::new(&config);
    let deployed = deployment(&ssm).await?;
    let store = Arc::new(DynamoStore::new(
        aws_sdk_dynamodb::Client::new(&config),
        deployed.truck_table,
        deployed.extraction_table,
    ));
    let media = Arc::new(S3Media::new(
        aws_sdk_s3::Client::new(&config),
        deployed.menu_bucket,
        deployed.identity.public_site.clone(),
    ));
    let public = Api::new(
        store.clone(),
        media.clone(),
        deployed.identity.clone(),
        Surface::Public,
    );
    let operator = Api::new(store, media, deployed.identity, Surface::Operator);
    lambda_http::run(service_fn(|request: Request| async {
        let surface = invoked_surface(&request.lambda_context().invoked_function_arn)?;
        let api = match surface {
            Surface::Public => public.clone(),
            Surface::Operator if request.uri().path().starts_with("/admin/") => operator
                .clone()
                .with_admin_subject(admin_subject(&ssm).await?),
            Surface::Operator => operator.clone(),
        };
        Ok::<_, Error>(api.handle(request, Utc::now()).await)
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lambda_identity_keeps_the_public_and_authenticated_surfaces_separate() {
        let prefix = "arn:aws:lambda:us-east-1:123456789012:function:";
        assert!(matches!(
            invoked_surface(&format!("{prefix}rolling-local-public")),
            Ok(Surface::Public)
        ));
        assert!(matches!(
            invoked_surface(&format!("{prefix}rolling-local-operator:2")),
            Ok(Surface::Operator)
        ));
        assert!(invoked_surface(&format!("{prefix}other-operator")).is_err());
    }

    #[test]
    fn deployment_settings_expose_only_public_login_configuration() {
        let deployed: Deployment = serde_json::from_value(serde_json::json!({
            "truckTable": "rolling-local-trucks",
            "extractionTable": "rolling-local-menu-items",
            "menuBucket": "rolling-local-menus",
            "publicSite": "https://rolling-local.test",
            "issuer": "https://auth.test/",
            "audience": "https://api.rolling-local.test",
            "connection": "ahara",
            "webClientId": "web",
            "iosClientId": "ios",
            "androidClientId": "android",
            "redirectUri": "rollinglocal://oauth"
        }))
        .unwrap();
        assert_eq!(deployed.identity.public_site, "https://rolling-local.test");
        assert!(deployed.identity.admin_subject.is_none());
        let login = serde_json::to_value(deployed.identity).unwrap();
        assert_eq!(login["webClientId"], "web");
        assert!(login.get("publicSite").is_none());
        assert!(login.get("adminSubject").is_none());
    }
}
