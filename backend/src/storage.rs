use crate::domain::{Profile, Session, Truck};
use async_trait::async_trait;
use aws_sdk_dynamodb::{
    Client,
    operation::transact_write_items::TransactWriteItemsError,
    types::{AttributeValue, ConditionCheck, Put, TransactWriteItem},
};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

type Attributes = HashMap<String, AttributeValue>;

#[derive(Debug, Clone)]
pub struct Record<T> {
    pub version: Uuid,
    pub value: T,
}

impl<T> Record<T> {
    pub fn new(value: T) -> Self {
        Self {
            version: Uuid::new_v4(),
            value,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StoreError {
    #[error("The record changed, retry the request")]
    Conflict,
    #[error("The storage service is unavailable")]
    Unavailable,
}

impl StoreError {
    pub fn unavailable(operation: &str) -> Self {
        tracing::error!(operation, "Storage operation failed");
        Self::Unavailable
    }
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn truck(&self, id: Uuid) -> Result<Option<Record<Truck>>, StoreError>;
    async fn owned_truck(&self, subject: &str) -> Result<Option<Record<Truck>>, StoreError>;
    async fn session(&self, truck: Uuid) -> Result<Option<Record<Session>>, StoreError>;
    async fn put_truck(
        &self,
        previous: Option<Uuid>,
        truck: Record<Truck>,
        link_owner: bool,
    ) -> Result<(), StoreError>;
    async fn put_session(
        &self,
        truck: Uuid,
        truck_version: Uuid,
        previous: Option<Uuid>,
        session: Record<Session>,
        start_command: bool,
    ) -> Result<(), StoreError>;
    async fn stop(&self, truck: Uuid, id: Uuid) -> Result<(), StoreError>;
    async fn directory(&self) -> Result<Vec<(Record<Truck>, Option<Session>)>, StoreError>;
}

pub struct DynamoStore {
    client: Client,
    table: String,
}

fn key(prefix: &str, id: impl std::fmt::Display) -> AttributeValue {
    AttributeValue::S(format!("{prefix}#{id}"))
}

fn item<T: Serialize>(key: AttributeValue, record: &Record<T>) -> Result<Attributes, StoreError> {
    let payload = serde_json::to_string(&record.value)
        .map_err(|_| StoreError::unavailable("encode_record"))?;
    Ok(HashMap::from([
        ("id".into(), key),
        (
            "revision".into(),
            AttributeValue::S(record.version.to_string()),
        ),
        ("payload".into(), AttributeValue::S(payload)),
    ]))
}

fn text<'a>(item: &'a Attributes, name: &str) -> Result<&'a str, StoreError> {
    item.get(name)
        .and_then(|v| v.as_s().ok())
        .map(String::as_str)
        .ok_or_else(|| StoreError::unavailable("read_record"))
}

fn decode<T: DeserializeOwned>(item: &Attributes) -> Result<Record<T>, StoreError> {
    Ok(Record {
        version: text(item, "revision")?
            .parse()
            .map_err(|_| StoreError::unavailable("read_revision"))?,
        value: serde_json::from_str(text(item, "payload")?)
            .map_err(|_| StoreError::unavailable("decode_record"))?,
    })
}

fn revision(previous: Option<Uuid>) -> (&'static str, Option<Attributes>) {
    match previous {
        Some(id) => (
            "revision = :prior",
            Some(HashMap::from([(
                ":prior".into(),
                AttributeValue::S(id.to_string()),
            )])),
        ),
        None => ("attribute_not_exists(id)", None),
    }
}

fn transaction_error(error: Option<&TransactWriteItemsError>, operation: &str) -> StoreError {
    if matches!(
        error,
        Some(TransactWriteItemsError::TransactionCanceledException(e)) if
            e.cancellation_reasons().iter().any(|reason| {
                matches!(
                    reason.code(),
                    Some("ConditionalCheckFailed" | "TransactionConflict")
                )
            })
    ) {
        StoreError::Conflict
    } else {
        StoreError::unavailable(operation)
    }
}

impl DynamoStore {
    pub fn new(client: Client, table: String) -> Self {
        Self { client, table }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        key: AttributeValue,
    ) -> Result<Option<Record<T>>, StoreError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table)
            .key("id", key)
            .consistent_read(true)
            .send()
            .await
            .map_err(|_| StoreError::unavailable("get_item"))?;
        result.item().map(decode).transpose()
    }

    fn condition_truck(&self, truck: Uuid, version: Uuid) -> Result<TransactWriteItem, StoreError> {
        let (condition, values) = revision(Some(version));
        let check = ConditionCheck::builder()
            .table_name(&self.table)
            .key("id", key("truck", truck))
            .condition_expression(condition)
            .set_expression_attribute_values(values)
            .build()
            .map_err(|_| StoreError::unavailable("truck_condition"))?;
        Ok(TransactWriteItem::builder().condition_check(check).build())
    }
}

#[async_trait]
impl Store for DynamoStore {
    async fn truck(&self, id: Uuid) -> Result<Option<Record<Truck>>, StoreError> {
        self.get(key("truck", id)).await
    }

    async fn owned_truck(&self, subject: &str) -> Result<Option<Record<Truck>>, StoreError> {
        let Some(binding) = self.get::<Uuid>(key("operator", subject)).await? else {
            return Ok(None);
        };
        self.truck(binding.value)
            .await?
            .filter(|record| record.value.operator_subject.as_deref() == Some(subject))
            .map(Some)
            .ok_or_else(|| StoreError::unavailable("operator_binding"))
    }

    async fn session(&self, truck: Uuid) -> Result<Option<Record<Session>>, StoreError> {
        self.get(key("live", truck)).await
    }

    async fn put_truck(
        &self,
        previous: Option<Uuid>,
        truck: Record<Truck>,
        link_owner: bool,
    ) -> Result<(), StoreError> {
        let (condition, values) = revision(previous);
        let data = item(key("truck", truck.value.id), &truck)?;
        if link_owner {
            let subject = truck
                .value
                .operator_subject
                .as_deref()
                .ok_or_else(|| StoreError::unavailable("owner_subject"))?;
            let binding = item(key("operator", subject), &Record::new(truck.value.id))?;
            let truck_put = Put::builder()
                .table_name(&self.table)
                .set_item(Some(data))
                .condition_expression(condition)
                .set_expression_attribute_values(values)
                .build()
                .map_err(|_| StoreError::unavailable("make_truck_write"))?;
            let binding_put = Put::builder()
                .table_name(&self.table)
                .set_item(Some(binding))
                .condition_expression("attribute_not_exists(id)")
                .build()
                .map_err(|_| StoreError::unavailable("make_binding_write"))?;
            return self
                .client
                .transact_write_items()
                .transact_items(TransactWriteItem::builder().put(truck_put).build())
                .transact_items(TransactWriteItem::builder().put(binding_put).build())
                .send()
                .await
                .map(|_| ())
                .map_err(|error| transaction_error(error.as_service_error(), "bind_operator"));
        }
        self.client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(data))
            .condition_expression(condition)
            .set_expression_attribute_values(values)
            .send()
            .await
            .map_err(|error| {
                if error
                    .as_service_error()
                    .is_some_and(|e| e.is_conditional_check_failed_exception())
                {
                    StoreError::Conflict
                } else {
                    StoreError::unavailable("put_truck")
                }
            })?;
        Ok(())
    }

    async fn put_session(
        &self,
        truck: Uuid,
        truck_version: Uuid,
        previous: Option<Uuid>,
        session: Record<Session>,
        start_command: bool,
    ) -> Result<(), StoreError> {
        let mut data = item(key("live", truck), &session)?;
        data.insert(
            "session_id".into(),
            AttributeValue::S(session.value.id.to_string()),
        );
        data.insert(
            "expires".into(),
            AttributeValue::N(session.value.location.expires_at.timestamp().to_string()),
        );
        let (condition, values) = revision(previous);
        let put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(data))
            .condition_expression(condition)
            .set_expression_attribute_values(values)
            .build()
            .map_err(|_| StoreError::unavailable("make_session_write"))?;
        let mut write = self
            .client
            .transact_write_items()
            .transact_items(self.condition_truck(truck, truck_version)?)
            .transact_items(TransactWriteItem::builder().put(put).build());
        if start_command {
            // A retried start must stay retired after an explicit stop.
            let guard = Put::builder()
                .table_name(&self.table)
                .condition_expression("attribute_not_exists(id)")
                .item(
                    "id",
                    AttributeValue::S(format!("start#{truck}#{}", session.value.id)),
                )
                .item(
                    "expires",
                    AttributeValue::N((session.value.published_at.timestamp() + 600).to_string()),
                )
                .build()
                .map_err(|_| StoreError::unavailable("make_start_guard"))?;
            write = write.transact_items(TransactWriteItem::builder().put(guard).build());
        }
        write
            .send()
            .await
            .map_err(|error| transaction_error(error.as_service_error(), "put_session"))?;
        Ok(())
    }

    async fn stop(&self, truck: Uuid, id: Uuid) -> Result<(), StoreError> {
        let delete_result = self
            .client
            .delete_item()
            .table_name(&self.table)
            .key("id", key("live", truck))
            .condition_expression("session_id = :session")
            .expression_attribute_values(":session", AttributeValue::S(id.to_string()))
            .send()
            .await;
        match delete_result {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.is_conditional_check_failed_exception()) =>
            {
                Ok(())
            }
            Err(_) => Err(StoreError::unavailable("stop_session")),
        }
    }

    async fn directory(&self) -> Result<Vec<(Record<Truck>, Option<Session>)>, StoreError> {
        let mut trucks = BTreeMap::new();
        let mut legacy = BTreeMap::new();
        let mut sessions = HashMap::new();
        let mut cursor = None;
        loop {
            let page = self
                .client
                .scan()
                .table_name(&self.table)
                .consistent_read(true)
                .set_exclusive_start_key(cursor)
                .send()
                .await
                .map_err(|_| StoreError::unavailable("public_directory"))?;
            for item in page.items() {
                if let Some((kind, id)) = text(item, "id")?.split_once('#') {
                    match kind {
                        "p" => {
                            let profile = decode::<Profile>(item)?;
                            legacy.insert(
                                profile.value.id.to_string(),
                                Record {
                                    version: profile.version,
                                    value: Truck::from_legacy(profile.value),
                                },
                            );
                        }
                        "truck" => {
                            trucks.insert(id.to_owned(), decode::<Truck>(item)?);
                        }
                        "live" => {
                            sessions.insert(id.to_owned(), decode::<Session>(item)?.value);
                        }
                        _ => {}
                    }
                }
            }
            cursor = page
                .last_evaluated_key()
                .filter(|key| !key.is_empty())
                .cloned();
            if cursor.is_none() {
                break;
            }
        }
        legacy.extend(trucks);
        Ok(legacy
            .into_iter()
            .map(|(id, profile)| (profile, sessions.remove(&id)))
            .collect())
    }
}
