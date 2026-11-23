mod admin;
pub mod domain;
pub mod extraction_worker;
pub mod media;
pub mod menus;
pub mod storage;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use domain::{
    Fix, Listing, Menu, Profile, PublicTruck, Rejection, Session, Settings, Sharing, Truck,
    TruckStatus,
};
use lambda_http::{
    Body, Request, Response,
    http::{Method, StatusCode},
};
use media::{Media, MediaError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::sync::Arc;
use storage::{Record, Store, StoreError};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub enum Surface {
    Public,
    Operator,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorConfig {
    pub issuer: String,
    pub audience: String,
    pub connection: String,
    pub web_client_id: String,
    pub ios_client_id: String,
    pub android_client_id: String,
    pub redirect_uri: String,
    #[serde(skip_serializing)]
    pub public_site: String,
    #[serde(skip)]
    pub admin_subject: Option<String>,
}

#[derive(Clone)]
pub struct Api {
    store: Arc<dyn Store>,
    media: Arc<dyn Media>,
    config: OperatorConfig,
    surface: Surface,
}

#[derive(Debug)]
struct Failure(StatusCode, String);

impl Failure {
    fn invalid(message: &str) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }

    fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "Sign in to Rolling Local".into())
    }

    fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "Endpoint not found".into())
    }

    fn forbidden(message: &str) -> Self {
        Self(StatusCode::FORBIDDEN, message.into())
    }
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        let status = match error {
            StoreError::Conflict => StatusCode::CONFLICT,
            StoreError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        Self(status, error.to_string())
    }
}

impl From<MediaError> for Failure {
    fn from(error: MediaError) -> Self {
        match error {
            MediaError::Invalid => Self::invalid(&error.to_string()),
            MediaError::Store(error) => error.into(),
        }
    }
}

impl From<Rejection> for Failure {
    fn from(error: Rejection) -> Self {
        let status = match error {
            Rejection::Closed | Rejection::Outside | Rejection::Ended => StatusCode::CONFLICT,
            Rejection::StaleFix | Rejection::InvalidFix => StatusCode::BAD_REQUEST,
        };
        Self(status, error.to_string())
    }
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: Audience,
    azp: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

impl Audience {
    fn contains(&self, expected: &str) -> bool {
        match self {
            Self::Single(value) => value == expected,
            Self::Multiple(values) => values.iter().any(|value| value == expected),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OperatorView<'a> {
    account_id: &'a str,
    registration: Option<Registration<'a>>,
    profile: Option<Profile>,
    session: Option<Sharing>,
}

#[derive(Serialize)]
struct Registration<'a> {
    id: Uuid,
    #[serde(flatten)]
    listing: &'a Listing,
    status: TruckStatus,
}

#[derive(Deserialize)]
struct Start {
    id: Uuid,
    fix: Fix,
}

#[derive(Deserialize)]
struct Upload {
    bytes: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Publish {
    id: Uuid,
    captured_at: DateTime<Utc>,
}

type Reply = Result<Response<Body>, Failure>;

fn response(status: StatusCode, value: impl Serialize) -> Response<Body> {
    let mut reply = Response::new(Body::Text(
        serde_json::to_string(&value).expect("API response is serializable"),
    ));
    *reply.status_mut() = status;
    let headers = reply.headers_mut();
    headers.insert(
        "content-type",
        "application/json; charset=utf-8".parse().unwrap(),
    );
    headers.insert("cache-control", "no-store".parse().unwrap());
    reply
}

fn body<T: DeserializeOwned>(req: &Request) -> Result<T, Failure> {
    serde_json::from_slice(req.body().as_ref())
        .map_err(|_| Failure::invalid("Invalid JSON request"))
}

fn ok(value: impl Serialize) -> Response<Body> {
    response(StatusCode::OK, value)
}

impl Api {
    pub fn new(
        store: Arc<dyn Store>,
        media: Arc<dyn Media>,
        config: OperatorConfig,
        surface: Surface,
    ) -> Self {
        Self {
            store,
            media,
            config,
            surface,
        }
    }

    pub fn with_admin_subject(mut self, subject: Option<String>) -> Self {
        self.config.admin_subject = subject;
        self
    }

    pub async fn handle(&self, req: Request, now: DateTime<Utc>) -> Response<Body> {
        let web_request = req
            .headers()
            .get("origin")
            .is_some_and(|origin| origin == self.config.public_site.as_str());
        if req.method() == Method::OPTIONS && matches!(self.surface, Surface::Public) {
            return self.preflight(&req, web_request);
        }
        let result = match self.surface {
            Surface::Public => self.public(&req, now).await,
            Surface::Operator => self.authenticated(&req, now).await,
        };
        let mut reply =
            result.unwrap_or_else(|failure| response(failure.0, json!({ "error": failure.1 })));
        if matches!(self.surface, Surface::Public) {
            reply
                .headers_mut()
                .insert("access-control-allow-origin", "*".parse().unwrap());
        } else {
            reply
                .headers_mut()
                .insert("vary", "Origin".parse().unwrap());
            if web_request {
                self.allow_site(&mut reply);
            }
        }
        reply
    }

    fn allow_site(&self, reply: &mut Response<Body>) {
        reply.headers_mut().insert(
            "access-control-allow-origin",
            self.config.public_site.parse().expect("Valid site origin"),
        );
    }

    fn preflight(&self, req: &Request, web_request: bool) -> Response<Body> {
        let path = req.uri().path();
        if !web_request
            || !(path == "/operator"
                || path.starts_with("/operator/")
                || path.starts_with("/admin/"))
        {
            return response(
                StatusCode::FORBIDDEN,
                json!({"error": "Origin not allowed"}),
            );
        }
        let mut reply = Response::new(Body::Empty);
        *reply.status_mut() = StatusCode::NO_CONTENT;
        self.allow_site(&mut reply);
        let headers = reply.headers_mut();
        headers.insert("vary", "Origin".parse().unwrap());
        headers.insert(
            "access-control-allow-methods",
            "GET, POST, PUT, DELETE".parse().unwrap(),
        );
        headers.insert(
            "access-control-allow-headers",
            "authorization, content-type".parse().unwrap(),
        );
        reply
    }

    async fn public(&self, req: &Request, now: DateTime<Utc>) -> Reply {
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/health") => Ok(ok(json!({ "status": "ok" }))),
            (&Method::GET, "/public/config") => Ok(ok(&self.config)),
            (&Method::GET, "/public/menu-catalog") => self.catalog(req).await,
            (&Method::GET, "/public/trucks") => {
                let trucks: Vec<_> = self
                    .store
                    .directory()
                    .await?
                    .into_iter()
                    .filter_map(|(truck, session)| {
                        PublicTruck::from_truck(truck.value, session.as_ref(), now)
                    })
                    .collect();
                Ok(ok(json!({ "trucks": trucks })))
            }
            _ => Err(Failure::not_found()),
        }
    }

    fn subject(&self, req: &Request) -> Result<String, Failure> {
        // The operator Lambda is reached through the ALB's authenticated listener rule.
        let author = req
            .headers()
            .get("authorization")
            .and_then(|header| header.to_str().ok())
            .ok_or_else(Failure::unauthorized)?;
        let (kind, token) = author.split_once(' ').ok_or_else(Failure::unauthorized)?;
        if !kind.eq_ignore_ascii_case("bearer") {
            return Err(Failure::unauthorized());
        }
        let part = token.split('.').nth(1).ok_or_else(Failure::unauthorized)?;
        let payload = URL_SAFE_NO_PAD
            .decode(part)
            .map_err(|_| Failure::unauthorized())?;
        let claims: Claims =
            serde_json::from_slice(&payload).map_err(|_| Failure::unauthorized())?;
        let clients = [
            &self.config.web_client_id,
            &self.config.ios_client_id,
            &self.config.android_client_id,
        ];
        if claims.iss != self.config.issuer
            || !claims.aud.contains(&self.config.audience)
            || !clients.contains(&&claims.azp)
            || claims.sub.trim().is_empty()
            || claims.sub.len() > 255
        {
            return Err(Failure::unauthorized());
        }
        Ok(claims.sub)
    }

    async fn authenticated(&self, req: &Request, now: DateTime<Utc>) -> Reply {
        let subject = self.subject(req)?;
        if req.uri().path().starts_with("/admin/") {
            self.admin(req, &subject, now).await
        } else {
            self.operator(req, &subject, now).await
        }
    }

    async fn operator(&self, req: &Request, subject: &str, now: DateTime<Utc>) -> Reply {
        if req.uri().path().starts_with("/operator/menu/") {
            return self.menu_request(req, subject, now).await;
        }
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/operator") => self.read_operator(subject, now).await,
            (&Method::POST, "/operator/registration") => {
                self.register(subject, body(req)?, now).await
            }
            (&Method::PUT, "/operator/settings") => self.settings(subject, body(req)?, now).await,
            (&Method::POST, "/operator/sessions") => self.start(subject, body(req)?, now).await,
            _ => self.session_request(req, subject, now).await,
        }
    }

    async fn menu_request(&self, req: &Request, subject: &str, now: DateTime<Utc>) -> Reply {
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/operator/menu/extraction") => self.extraction(subject).await,
            (&Method::PUT, "/operator/menu/extraction") => self.correct_menu(subject, req).await,
            (&Method::POST, "/operator/menu/extraction/retry") => {
                self.retry_menu(subject, req).await
            }
            (&Method::POST, "/operator/menu/upload") => {
                let truck = self.approved(subject).await?;
                let data: Upload = body(req)?;
                Ok(ok(self.media.upload(truck.value.id, data.bytes).await?))
            }
            (&Method::POST, "/operator/menu/publish") => {
                self.publish(subject, body(req)?, now).await
            }
            _ => Err(Failure::not_found()),
        }
    }

    async fn session_request(&self, req: &Request, subject: &str, now: DateTime<Utc>) -> Reply {
        let path = req
            .uri()
            .path()
            .strip_prefix("/operator/sessions/")
            .ok_or_else(Failure::not_found)?;
        match (req.method(), path.strip_suffix("/fix")) {
            (&Method::PUT, Some(id)) => {
                self.fix(
                    subject,
                    id.parse().map_err(|_| Failure::not_found())?,
                    body(req)?,
                    now,
                )
                .await
            }
            (&Method::DELETE, None) => {
                let truck = self.approved(subject).await?;
                self.store
                    .stop(
                        truck.value.id,
                        path.parse().map_err(|_| Failure::not_found())?,
                    )
                    .await?;
                Ok(ok(json!({ "session": null })))
            }
            _ => Err(Failure::not_found()),
        }
    }

    async fn approved(&self, subject: &str) -> Result<Record<Truck>, Failure> {
        self.store
            .owned_truck(subject)
            .await?
            .filter(|record| record.value.status == TruckStatus::Approved)
            .ok_or_else(|| Failure::forbidden("An approved truck is required"))
    }

    async fn read_operator(&self, subject: &str, now: DateTime<Utc>) -> Reply {
        let truck = self
            .store
            .owned_truck(subject)
            .await?
            .map(|record| record.value);
        let profile = truck.as_ref().and_then(Truck::operator_profile);
        let session = match &profile {
            Some(profile) => self
                .store
                .session(profile.id)
                .await?
                .filter(|record| record.value.active(profile, now))
                .map(|record| Sharing::from(&record.value)),
            None => None,
        };
        let registration = truck.as_ref().map(|truck| Registration {
            id: truck.id,
            listing: &truck.listing,
            status: truck.status,
        });
        Ok(ok(OperatorView {
            account_id: subject,
            registration,
            profile,
            session,
        }))
    }

    async fn register(&self, subject: &str, listing: Listing, now: DateTime<Utc>) -> Reply {
        let listing = listing
            .normalize()
            .ok_or_else(|| Failure::invalid("Enter a truck name and cuisine"))?;
        if self.store.owned_truck(subject).await?.is_none() {
            let truck = Truck::new(listing, Some(subject.into()));
            let result = self.store.put_truck(None, Record::new(truck), true).await;
            if result == Err(StoreError::Conflict)
                && self.store.owned_truck(subject).await?.is_some()
            {
                return self.read_operator(subject, now).await;
            }
            result?;
        }
        self.read_operator(subject, now).await
    }

    async fn settings(&self, subject: &str, settings: Settings, now: DateTime<Utc>) -> Reply {
        if !settings.valid(now) {
            return Err(Failure::invalid(
                "Check your truck name, cuisine, hours, and operating areas",
            ));
        }
        let mut prior = self.approved(subject).await?;
        prior.value.update_settings(settings);
        self.store
            .put_truck(Some(prior.version), Record::new(prior.value), false)
            .await?;
        self.read_operator(subject, now).await
    }

    async fn start(&self, subject: &str, start: Start, now: DateTime<Utc>) -> Reply {
        let truck = self.approved(subject).await?;
        let profile = truck
            .value
            .operator_profile()
            .ok_or_else(|| Failure::invalid("Complete operating settings first"))?;
        let prior = self.store.session(profile.id).await?;
        if let Some(current) = prior
            .as_ref()
            .filter(|current| current.value.id == start.id)
        {
            if current.value.active(&profile, now) {
                return Ok(ok(Sharing::from(&current.value)));
            }
            return Err(Rejection::Ended.into());
        }
        let session = Session::begin(&profile, start.id, start.fix, now)?;
        let sharing = Sharing::from(&session);
        self.store
            .put_session(
                profile.id,
                truck.version,
                prior.map(|r| r.version),
                Record::new(session),
                true,
            )
            .await?;
        Ok(ok(sharing))
    }

    async fn fix(&self, subject: &str, id: Uuid, fix: Fix, now: DateTime<Utc>) -> Reply {
        let truck = self.approved(subject).await?;
        let profile = truck.value.operator_profile().ok_or(Rejection::Ended)?;
        let mut current = self
            .store
            .session(profile.id)
            .await?
            .filter(|r| r.value.id == id)
            .ok_or(Rejection::Ended)?;
        match current.value.update(&profile, fix, now) {
            Ok(true) => {
                let sharing = Sharing::from(&current.value);
                self.store
                    .put_session(
                        profile.id,
                        truck.version,
                        Some(current.version),
                        Record::new(current.value),
                        false,
                    )
                    .await?;
                Ok(ok(sharing))
            }
            Ok(false) => Ok(ok(Sharing::from(&current.value))),
            Err(
                reason @ (Rejection::Outside
                | Rejection::Closed
                | Rejection::Ended
                | Rejection::InvalidFix),
            ) => {
                self.store.stop(profile.id, id).await?;
                Err(reason.into())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn publish(&self, subject: &str, publish: Publish, now: DateTime<Utc>) -> Reply {
        if publish.captured_at > now + Duration::minutes(1) {
            return Err(Failure::invalid("Capture time must not be in the future"));
        }
        let mut truck = self.approved(subject).await?;
        if let Some(menu) = &truck.value.menu {
            if menu.id == publish.id {
                return Ok(ok(menu));
            }
            if menu.captured_at > publish.captured_at {
                return Err(Failure(
                    StatusCode::CONFLICT,
                    "A newer menu is already published".into(),
                ));
            }
        }
        let url = self.media.publish(truck.value.id, publish.id).await?;
        let menu = Menu {
            id: publish.id,
            captured_at: publish.captured_at,
            published_at: now,
            url,
        };
        truck.value.menu = Some(menu.clone());
        let extraction = Record::new(menus::Extraction::queued(truck.value.id, menu.id));
        self.store
            .publish_menu(truck.version, Record::new(truck.value), extraction)
            .await?;
        Ok(ok(menu))
    }
}
