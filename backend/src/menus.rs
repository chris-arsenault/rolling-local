use crate::{
    Api, Failure, Reply, body,
    domain::TruckStatus,
    ok,
    storage::{Record, StoreError},
};
use chrono::{DateTime, Utc};
use lambda_http::{Request, RequestExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use uuid::Uuid;

pub const TAG_IDS: &[&str] = &[
    "american",
    "mexican",
    "japanese",
    "italian",
    "chinese",
    "korean",
    "thai",
    "indian",
    "mediterranean",
    "middle-eastern",
    "tacos",
    "burritos",
    "sushi",
    "ramen",
    "pizza",
    "burgers",
    "hot-dogs",
    "bbq",
    "sandwiches",
    "dumplings",
    "coffee",
    "desserts",
    "hot",
    "vegan",
    "vegetarian",
];

fn tags() -> Vec<Value> {
    TAG_IDS
        .iter()
        .map(|&id| {
            let (label, aliases): (String, &[&str]) = match id {
                "hot" => ("Hot / spicy".into(), &["spicy"]),
                "hot-dogs" => ("Hot Dogs".into(), &["hot dog"]),
                "bbq" => ("BBQ".into(), &["barbecue", "barbeque"]),
                "middle-eastern" => ("Middle Eastern".into(), &[]),
                _ => (id[..1].to_uppercase() + &id[1..], &[]),
            };
            json!({"id":id,"label":label,"aliases":aliases})
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Processing,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fields {
    pub name: String,
    pub description: Option<String>,
    pub price_text: Option<String>,
    pub tag_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: Uuid,
    pub position: usize,
    #[serde(flatten)]
    pub fields: Fields,
}

fn search_text(text: &str) -> String {
    let folded: String = text
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogItem {
    #[serde(flatten)]
    item: MenuItem,
    search_name: String,
    search_description: Option<String>,
}

impl From<MenuItem> for CatalogItem {
    fn from(item: MenuItem) -> Self {
        Self {
            search_name: search_text(&item.fields.name),
            search_description: item.fields.description.as_deref().map(search_text),
            item,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub truck_id: Uuid,
    pub menu_id: Uuid,
    pub job_id: Uuid,
    pub status: Status,
    pub schema_version: u8,
    pub tag_vocabulary_version: u8,
    pub extracted_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
    pub items: Vec<MenuItem>,
}

impl Extraction {
    pub fn queued(truck_id: Uuid, menu_id: Uuid) -> Self {
        Self {
            truck_id,
            menu_id,
            job_id: menu_id,
            status: Status::Queued,
            schema_version: 1,
            tag_vocabulary_version: 1,
            extracted_at: None,
            error_code: None,
            items: vec![],
        }
    }
}

pub fn validate(items: &mut [MenuItem]) -> Result<(), &'static str> {
    let mut ids = HashSet::new();
    for (position, item) in items.iter_mut().enumerate() {
        let fields = &item.fields;
        if !ids.insert(item.id) || fields.name.trim().is_empty() || fields.name.len() > 512 {
            return Err("Each menu item needs a unique ID and a readable name");
        }
        if fields.description.as_ref().is_some_and(|s| s.len() > 4096)
            || fields.price_text.as_ref().is_some_and(|s| s.len() > 512)
        {
            return Err("Menu description or price is too long");
        }
        let mut tags = HashSet::new();
        if !fields
            .tag_ids
            .iter()
            .all(|s| TAG_IDS.contains(&s.as_str()) && tags.insert(s))
        {
            return Err("Choose distinct tags from the menu vocabulary");
        }
        item.position = position;
    }
    // Leave room for search fields in the public response and for the ALB envelope.
    if serde_json::to_vec(items)
        .map_err(|_| "Invalid menu items")?
        .len()
        > 180_000
    {
        return Err("The structured menu is too large");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Correction {
    menu_id: Uuid,
    revision: Option<Uuid>,
    items: Vec<MenuItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Retry {
    menu_id: Uuid,
    revision: Uuid,
}

fn operator_result(record: Option<Record<Extraction>>) -> Value {
    let extraction = record.map(|entry| {
        let mut data = json!(entry.value);
        data["revision"] = json!(entry.version);
        data
    });
    json!({"extraction":extraction, "tags":tags()})
}

impl Api {
    pub(crate) async fn retry_menu(&self, subject: &str, req: &Request) -> Reply {
        let retry: Retry = body(req)?;
        let truck = self.approved(subject).await?;
        let menu = truck
            .value
            .menu
            .filter(|m| m.id == retry.menu_id)
            .ok_or(StoreError::Conflict)?;
        let prior = self
            .store
            .extraction(truck.value.id, menu.id)
            .await?
            .filter(|r| r.version == retry.revision && r.value.status == Status::Failed)
            .ok_or(StoreError::Conflict)?;
        let mut queued = prior.value;
        queued.job_id = Uuid::new_v4();
        queued.status = Status::Queued;
        queued.error_code = None;
        let record = Record::new(queued);
        self.store
            .write_extraction(&record, Some(retry.revision), Some(truck.version))
            .await?;
        Ok(ok(operator_result(Some(record))))
    }
    pub(crate) async fn extraction(&self, subject: &str) -> Reply {
        let truck = self.approved(subject).await?;
        let record = match truck.value.menu {
            Some(menu) => self.store.extraction(truck.value.id, menu.id).await?,
            None => None,
        };
        Ok(ok(operator_result(record)))
    }

    pub(crate) async fn correct_menu(&self, subject: &str, req: &Request) -> Reply {
        let truck = self.approved(subject).await?;
        let mut correction: Correction = body(req)?;
        let menu = truck.value.menu.ok_or(StoreError::Conflict)?;
        if menu.id != correction.menu_id {
            return Err(StoreError::Conflict.into());
        }
        validate(&mut correction.items).map_err(Failure::invalid)?;
        let prior = self.store.extraction(truck.value.id, menu.id).await?;
        if prior.as_ref().map(|r| r.version) != correction.revision {
            return Err(StoreError::Conflict.into());
        }
        let mut value = prior
            .map(|r| r.value)
            .unwrap_or_else(|| Extraction::queued(truck.value.id, menu.id));
        value.items = correction.items;
        value.status = Status::Ready;
        value.error_code = None;
        let record = Record::new(value);
        self.store
            .write_extraction(&record, correction.revision, Some(truck.version))
            .await?;
        Ok(ok(operator_result(Some(record))))
    }

    pub(crate) async fn catalog(&self, req: &Request) -> Reply {
        let cursor = req
            .query_string_parameters()
            .first("after")
            .map(str::parse::<Uuid>)
            .transpose()
            .map_err(|_| Failure::invalid("Invalid catalog cursor"))?;
        let mut listings: Vec<_> = self
            .store
            .directory()
            .await?
            .into_iter()
            .map(|(r, _)| r.value)
            .filter(|t| t.status == TruckStatus::Approved && cursor.is_none_or(|id| t.id > id))
            .filter_map(|t| t.menu.map(|m| (t.id, m)))
            .collect();
        listings.sort_by_key(|(id, _)| *id);
        let mut last_included = None;
        let mut next = None;
        let mut bytes = 0;
        let mut menus = Vec::new();
        for (truck_id, menu) in listings {
            let record = self.store.extraction(truck_id, menu.id).await?;
            let status = record.as_ref().map(|r| r.value.status);
            let items = record
                .filter(|r| r.value.status == Status::Ready)
                .map(|r| {
                    r.value
                        .items
                        .into_iter()
                        .map(CatalogItem::from)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let entry = json!({"truckId":truck_id,"menuId":menu.id,"capturedAt":menu.captured_at,
                "publishedAt":menu.published_at,"url":menu.url,"status":status,"items":items});
            bytes += entry.to_string().len();
            // The ALB wraps and escapes the response body within its 1 MiB limit.
            if bytes > 450_000 && last_included.is_some() {
                next = last_included;
                break;
            }
            menus.push(entry);
            last_included = Some(truck_id);
        }
        Ok(ok(
            json!({"menus":menus,"next":next,"tagVocabularyVersion":1,"tags":tags()}),
        ))
    }
}
