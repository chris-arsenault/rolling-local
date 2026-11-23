use crate::{
    Api, Failure, Reply, body,
    domain::{Listing, Review, Truck, TruckStatus},
    ok,
    storage::{Record, StoreError},
};
use chrono::{DateTime, Utc};
use lambda_http::{Request, http::Method};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminTruck<'a> {
    id: Uuid,
    version: Uuid,
    #[serde(flatten)]
    listing: &'a Listing,
    status: TruckStatus,
    operator_subject: Option<&'a str>,
    review: Option<&'a Review>,
}

impl<'a> From<&'a Record<Truck>> for AdminTruck<'a> {
    fn from(record: &'a Record<Truck>) -> Self {
        Self {
            id: record.value.id,
            version: record.version,
            listing: &record.value.listing,
            status: record.value.status,
            operator_subject: record.value.operator_subject.as_deref(),
            review: record.value.review.as_ref(),
        }
    }
}

#[derive(Deserialize)]
struct Create {
    id: Uuid,
    #[serde(flatten)]
    listing: Listing,
    publish: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Update {
    version: Uuid,
    listing: Option<Listing>,
    decision: Option<Decision>,
    operator_subject: Option<String>,
    confirm_ownership: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Decision {
    Accept,
    Reject,
}

fn review(truck: &mut Truck, decision: Decision, by: &str, at: DateTime<Utc>) {
    truck.status = match decision {
        Decision::Accept => TruckStatus::Approved,
        Decision::Reject => TruckStatus::Rejected,
    };
    truck.review = Some(Review { by: by.into(), at });
}

fn attach_operator(truck: &mut Truck, subject: Option<String>) -> Result<bool, Failure> {
    let Some(subject) = subject else {
        return Ok(false);
    };
    if subject.trim().is_empty() || subject.len() > 255 || subject != subject.trim() {
        return Err(Failure::invalid("Enter the operator's exact Auth0 user ID"));
    }
    if truck.operator_subject.is_some() || truck.status == TruckStatus::Rejected {
        return Err(Failure::invalid("Only an unassigned truck can be linked"));
    }
    truck.operator_subject = Some(subject);
    Ok(true)
}

impl Api {
    pub(crate) async fn admin(&self, req: &Request, subject: &str, now: DateTime<Utc>) -> Reply {
        if self.config.admin_subject.as_deref() != Some(subject) {
            return Err(Failure::forbidden(
                "Truck administration is restricted to the owner",
            ));
        }
        let path = req.uri().path();
        match (req.method(), path) {
            (&Method::GET, "/admin/trucks") => {
                let rows = self.store.directory().await?;
                let trucks: Vec<_> = rows.iter().map(|(row, _)| AdminTruck::from(row)).collect();
                Ok(ok(json!({ "trucks": trucks })))
            }
            (&Method::POST, "/admin/trucks") => self.create_truck(body(req)?, subject, now).await,
            (&Method::PUT, path) => {
                let id = path
                    .strip_prefix("/admin/trucks/")
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(Failure::not_found)?;
                self.update_truck(id, body(req)?, subject, now).await
            }
            _ => Err(Failure::not_found()),
        }
    }

    async fn create_truck(&self, data: Create, subject: &str, now: DateTime<Utc>) -> Reply {
        let listing = data
            .listing
            .normalize()
            .ok_or_else(|| Failure::invalid("Enter a truck name and cuisine"))?;
        let mut truck = Truck::new(listing, None);
        truck.id = data.id;
        if data.publish {
            review(&mut truck, Decision::Accept, subject, now);
        }
        let record = Record::new(truck);
        match self.store.put_truck(None, record.clone(), false).await {
            Ok(()) => Ok(ok(AdminTruck::from(&record))),
            Err(StoreError::Conflict) => {
                let current = self
                    .store
                    .truck(data.id)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                Ok(ok(AdminTruck::from(&current)))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn update_truck(
        &self,
        id: Uuid,
        data: Update,
        subject: &str,
        now: DateTime<Utc>,
    ) -> Reply {
        let (mut record, previous) = self.editable_truck(id).await?;
        if record.version != data.version {
            return Err(StoreError::Conflict.into());
        }
        if let Some(listing) = data.listing {
            record.value.listing = listing
                .normalize()
                .ok_or_else(|| Failure::invalid("Enter a truck name and cuisine"))?;
        }
        if let Some(decision) = data.decision {
            if record.value.status != TruckStatus::Pending {
                return Err(StoreError::Conflict.into());
            }
            review(&mut record.value, decision, subject, now);
        }
        if data.operator_subject.is_some() && data.confirm_ownership != Some(true) {
            return Err(Failure::invalid(
                "Confirm the operator’s identity before linking",
            ));
        }
        let link = attach_operator(&mut record.value, data.operator_subject)?;
        let record = Record::new(record.value);
        self.store.put_truck(previous, record.clone(), link).await?;
        Ok(ok(AdminTruck::from(&record)))
    }

    async fn editable_truck(&self, id: Uuid) -> Result<(Record<Truck>, Option<Uuid>), Failure> {
        if let Some(record) = self.store.truck(id).await? {
            let previous = record.version;
            return Ok((record, Some(previous)));
        }
        // Pre-Auth0 profiles have their own storage keys. Preserve their public IDs
        // and copy them into a canonical truck row on the first owner edit.
        self.store
            .directory()
            .await?
            .into_iter()
            .find(|(row, _)| row.value.id == id)
            .map(|(row, _)| (row, None))
            .ok_or_else(Failure::not_found)
    }
}
