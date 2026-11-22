use crate::domain::{Profile, Session};
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
    async fn profile(&self, owner: Uuid) -> Result<Option<Record<Profile>>, StoreError>;
    async fn session(&self, owner: Uuid) -> Result<Option<Record<Session>>, StoreError>;
    async fn put_profile(
        &self,
        owner: Uuid,
        previous: Option<Uuid>,
        profile: Record<Profile>,
    ) -> Result<(), StoreError>;
    async fn put_session(
        &self,
        owner: Uuid,
        profile_version: Uuid,
        previous: Option<Uuid>,
        session: Record<Session>,
        start_command: bool,
    ) -> Result<(), StoreError>;
    async fn stop(&self, owner: Uuid, id: Uuid) -> Result<(), StoreError>;
    async fn directory(&self) -> Result<Vec<(Profile, Option<Session>)>, StoreError>;
}

pub struct DynamoStore {
    client: Client,
    table: String,
}

fn key(prefix: &str, owner: Uuid) -> AttributeValue {
    AttributeValue::S(format!("{prefix}#{owner}"))
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

    fn condition_profile(
        &self,
        owner: Uuid,
        version: Uuid,
    ) -> Result<TransactWriteItem, StoreError> {
        let (condition, values) = revision(Some(version));
        let check = ConditionCheck::builder()
            .table_name(&self.table)
            .key("id", key("p", owner))
            .condition_expression(condition)
            .set_expression_attribute_values(values)
            .build()
            .map_err(|_| StoreError::unavailable("profile_condition"))?;
        Ok(TransactWriteItem::builder().condition_check(check).build())
    }
}

#[async_trait]
impl Store for DynamoStore {
    async fn profile(&self, owner: Uuid) -> Result<Option<Record<Profile>>, StoreError> {
        self.get(key("p", owner)).await
    }

    async fn session(&self, owner: Uuid) -> Result<Option<Record<Session>>, StoreError> {
        self.get(key("s", owner)).await
    }

    async fn put_profile(
        &self,
        owner: Uuid,
        previous: Option<Uuid>,
        profile: Record<Profile>,
    ) -> Result<(), StoreError> {
        let (condition, values) = revision(previous);
        self.client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(item(key("p", owner), &profile)?))
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
                    StoreError::unavailable("put_profile")
                }
            })?;
        Ok(())
    }

    async fn put_session(
        &self,
        owner: Uuid,
        profile_version: Uuid,
        previous: Option<Uuid>,
        session: Record<Session>,
        start_command: bool,
    ) -> Result<(), StoreError> {
        let mut data = item(key("s", owner), &session)?;
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
            .transact_items(self.condition_profile(owner, profile_version)?)
            .transact_items(TransactWriteItem::builder().put(put).build());
        if start_command {
            // A retried start must stay retired after an explicit stop.
            let guard = Put::builder()
                .table_name(&self.table)
                .condition_expression("attribute_not_exists(id)")
                .item(
                    "id",
                    AttributeValue::S(format!("start#{owner}#{}", session.value.id)),
                )
                .item(
                    "expires",
                    AttributeValue::N((session.value.published_at.timestamp() + 600).to_string()),
                )
                .build()
                .map_err(|_| StoreError::unavailable("make_start_guard"))?;
            write = write.transact_items(TransactWriteItem::builder().put(guard).build());
        }
        write.send().await.map_err(|error| {
            if matches!(
                error.as_service_error(),
                Some(TransactWriteItemsError::TransactionCanceledException(e)) if {
                    e.cancellation_reasons().iter().any(|reason| {
                        matches!(
                            reason.code(),
                            Some("ConditionalCheckFailed" | "TransactionConflict")
                        )
                    })
                }
            ) {
                StoreError::Conflict
            } else {
                StoreError::unavailable("put_session")
            }
        })?;
        Ok(())
    }

    async fn stop(&self, owner: Uuid, id: Uuid) -> Result<(), StoreError> {
        let delete_result = self
            .client
            .delete_item()
            .table_name(&self.table)
            .key("id", key("s", owner))
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

    async fn directory(&self) -> Result<Vec<(Profile, Option<Session>)>, StoreError> {
        let mut profiles = BTreeMap::new();
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
                if let Some((kind, owner)) = text(item, "id")?.split_once('#') {
                    match kind {
                        "p" => {
                            profiles.insert(owner.to_owned(), decode::<Profile>(item)?.value);
                        }
                        "s" => {
                            sessions.insert(owner.to_owned(), decode::<Session>(item)?.value);
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
        Ok(profiles
            .into_iter()
            .map(|(id, profile)| (profile, sessions.remove(&id)))
            .collect())
    }
}
