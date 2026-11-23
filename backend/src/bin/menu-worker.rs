use lambda_runtime::{Error, LambdaEvent, service_fn};
use rolling_local_api::{
    extraction_worker::{BedrockReader, process},
    storage::DynamoStore,
};
use serde::Deserialize;
use std::env;
use uuid::Uuid;

#[derive(Deserialize)]
struct Batch {
    #[serde(rename = "Records")]
    records: Vec<Change>,
}
#[derive(Deserialize)]
struct Change {
    dynamodb: StreamRecord,
}
#[derive(Deserialize)]
struct StreamRecord {
    #[serde(rename = "NewImage")]
    image: Dispatch,
}
#[derive(Deserialize)]
struct Dispatch {
    #[serde(rename = "truckId")]
    truck: StringKey,
    id: StringKey,
    dispatch: StringKey,
}
#[derive(Deserialize)]
struct StringKey {
    #[serde(rename = "S")]
    value: Uuid,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt().json().init();
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "TLS provider unavailable")?;
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store = DynamoStore::new(
        aws_sdk_dynamodb::Client::new(&config),
        env::var("TRUCK_TABLE")?,
        env::var("EXTRACTION_TABLE")?,
    );
    let reader = BedrockReader {
        runtime: aws_sdk_bedrockruntime::Client::new(&config),
        s3: aws_sdk_s3::Client::new(&config),
        bucket: env::var("MENU_BUCKET")?,
        model: env::var("BEDROCK_MODEL_ID")?,
    };
    lambda_runtime::run(service_fn(|event: LambdaEvent<Batch>| async {
        for record in event.payload.records {
            let image = record.dynamodb.image;
            process(
                &store,
                &reader,
                image.truck.value,
                image.id.value,
                image.dispatch.value,
            )
            .await?;
        }
        Ok::<(), Error>(())
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_images_identify_the_menu_and_the_requested_attempt() {
        let menu = Uuid::new_v4();
        let job = Uuid::new_v4();
        let event = serde_json::json!({"Records":[{"eventName":"MODIFY","dynamodb":{
            "NewImage":{"truckId":{"S":Uuid::new_v4()},"id":{"S":menu},"dispatch":{"S":job},"payload":{"S":"{}"}}}}]});
        let record = serde_json::from_value::<Batch>(event)
            .unwrap()
            .records
            .remove(0);
        assert_eq!(record.dynamodb.image.id.value, menu);
        assert_eq!(record.dynamodb.image.dispatch.value, job);
    }
}
