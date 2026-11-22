use chrono::Utc;
use lambda_http::{Error, service_fn};
use rolling_local_api::{Api, OperatorConfig, Surface, media::S3Media, storage::DynamoStore};
use std::{env, sync::Arc};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Error> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "Could not install the TLS crypto provider")?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store = DynamoStore::new(
        aws_sdk_dynamodb::Client::new(&config),
        env::var("TRUCK_TABLE")?,
    );
    let media = S3Media::new(
        aws_sdk_s3::Client::new(&config),
        env::var("MENU_BUCKET")?,
        env::var("PUBLIC_SITE")?,
    );
    let identity = OperatorConfig {
        cognito_domain: env::var("COGNITO_DOMAIN")?,
        client_id: env::var("COGNITO_CLIENT_ID")?,
        redirect_uri: env::var("MOBILE_REDIRECT_URI")?,
    };
    let surface = match env::var("SURFACE")?.as_str() {
        "public" => Surface::Public,
        "operator" => Surface::Operator,
        _ => return Err("Unexpected application surface".into()),
    };
    let api = Api::new(Arc::new(store), Arc::new(media), identity, surface);
    lambda_http::run(service_fn(|request| async {
        Ok::<_, Error>(api.handle(request, Utc::now()).await)
    }))
    .await
}
