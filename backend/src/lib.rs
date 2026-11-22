pub mod domain;
pub mod media;
pub mod storage;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use domain::{Fix, Menu, Profile, PublicTruck, Rejection, Session, Settings, Sharing};
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorConfig {
    pub cognito_domain: String,
    pub client_id: String,
    pub redirect_uri: String,
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
    sub: Uuid,
    client_id: String,
    token_use: String,
}

#[derive(Serialize)]
struct OperatorView {
    profile: Option<Profile>,
    session: Option<Sharing>,
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

    pub async fn handle(&self, req: Request, now: DateTime<Utc>) -> Response<Body> {
        let result = match self.surface {
            Surface::Public => self.public(&req, now).await,
            Surface::Operator => self.operator(&req, now).await,
        };
        let mut reply =
            result.unwrap_or_else(|failure| response(failure.0, json!({ "error": failure.1 })));
        if matches!(self.surface, Surface::Public) {
            reply
                .headers_mut()
                .insert("access-control-allow-origin", "*".parse().unwrap());
        }
        reply
    }

    async fn public(&self, req: &Request, now: DateTime<Utc>) -> Reply {
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/health") => Ok(ok(json!({ "status": "ok" }))),
            (&Method::GET, "/public/config") => Ok(ok(&self.config)),
            (&Method::GET, "/public/trucks") => {
                let trucks: Vec<_> = self
                    .store
                    .directory()
                    .await?
                    .into_iter()
                    .map(|(profile, session)| {
                        PublicTruck::from_profile(profile, session.as_ref(), now)
                    })
                    .collect();
                Ok(ok(json!({ "trucks": trucks })))
            }
            _ => Err(Failure::not_found()),
        }
    }

    fn owner(&self, req: &Request) -> Result<Uuid, Failure> {
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
        if claims.token_use != "access" || claims.client_id != self.config.client_id {
            return Err(Failure::unauthorized());
        }
        Ok(claims.sub)
    }

    async fn operator(&self, req: &Request, now: DateTime<Utc>) -> Reply {
        let owner = self.owner(req)?;
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/operator") => self.read_operator(owner, now).await,
            (&Method::PUT, "/operator/settings") => self.settings(owner, body(req)?, now).await,
            (&Method::POST, "/operator/sessions") => self.start(owner, body(req)?, now).await,
            (&Method::POST, "/operator/menu/upload") => {
                let profile = self.enrolled(owner).await?;
                let data: Upload = body(req)?;
                Ok(ok(self.media.upload(profile.value.id, data.bytes).await?))
            }
            (&Method::POST, "/operator/menu/publish") => self.publish(owner, body(req)?, now).await,
            _ => self.session_request(req, owner, now).await,
        }
    }

    async fn session_request(&self, req: &Request, owner: Uuid, now: DateTime<Utc>) -> Reply {
        let path = req
            .uri()
            .path()
            .strip_prefix("/operator/sessions/")
            .ok_or_else(Failure::not_found)?;
        match (req.method(), path.strip_suffix("/fix")) {
            (&Method::PUT, Some(id)) => {
                self.fix(
                    owner,
                    id.parse().map_err(|_| Failure::not_found())?,
                    body(req)?,
                    now,
                )
                .await
            }
            (&Method::DELETE, None) => {
                self.store
                    .stop(owner, path.parse().map_err(|_| Failure::not_found())?)
                    .await?;
                Ok(ok(json!({ "session": null })))
            }
            _ => Err(Failure::not_found()),
        }
    }

    async fn enrolled(&self, owner: Uuid) -> Result<Record<Profile>, Failure> {
        self.store
            .profile(owner)
            .await?
            .ok_or_else(|| Failure::invalid("Complete truck setup first"))
    }

    async fn read_operator(&self, owner: Uuid, now: DateTime<Utc>) -> Reply {
        let profile = self.store.profile(owner).await?.map(|record| record.value);
        let session = match &profile {
            Some(profile) => self
                .store
                .session(owner)
                .await?
                .filter(|record| record.value.active(profile, now))
                .map(|record| Sharing::from(&record.value)),
            None => None,
        };
        Ok(ok(OperatorView { profile, session }))
    }

    async fn settings(&self, owner: Uuid, mut settings: Settings, now: DateTime<Utc>) -> Reply {
        if !settings.valid(now) {
            return Err(Failure::invalid(
                "Check your truck name, cuisine, hours, and operating areas",
            ));
        }
        settings.name = settings.name.trim().into();
        settings.cuisine = settings.cuisine.trim().into();
        settings.hours.weekdays.sort_unstable();
        settings.hours.weekdays.dedup();
        let prior = self.store.profile(owner).await?;
        let profile = match prior.as_ref() {
            Some(record) => Profile {
                id: record.value.id,
                policy_id: if record.value.settings.same_sharing_policy(&settings) {
                    record.value.policy_id
                } else {
                    Uuid::new_v4()
                },
                settings,
                menu: record.value.menu.clone(),
            },
            None => Profile {
                id: Uuid::new_v4(),
                policy_id: Uuid::new_v4(),
                settings,
                menu: None,
            },
        };
        self.store
            .put_profile(owner, prior.map(|r| r.version), Record::new(profile))
            .await?;
        self.read_operator(owner, now).await
    }

    async fn start(&self, owner: Uuid, start: Start, now: DateTime<Utc>) -> Reply {
        let profile = self.enrolled(owner).await?;
        let prior = self.store.session(owner).await?;
        if let Some(current) = prior
            .as_ref()
            .filter(|current| current.value.id == start.id)
        {
            if current.value.active(&profile.value, now) {
                return Ok(ok(Sharing::from(&current.value)));
            }
            return Err(Rejection::Ended.into());
        }
        let session = Session::begin(&profile.value, start.id, start.fix, now)?;
        let sharing = Sharing::from(&session);
        self.store
            .put_session(
                owner,
                profile.version,
                prior.map(|r| r.version),
                Record::new(session),
                true,
            )
            .await?;
        Ok(ok(sharing))
    }

    async fn fix(&self, owner: Uuid, id: Uuid, fix: Fix, now: DateTime<Utc>) -> Reply {
        let profile = self.enrolled(owner).await?;
        let mut current = self
            .store
            .session(owner)
            .await?
            .filter(|r| r.value.id == id)
            .ok_or(Rejection::Ended)?;
        match current.value.update(&profile.value, fix, now) {
            Ok(true) => {
                let sharing = Sharing::from(&current.value);
                self.store
                    .put_session(
                        owner,
                        profile.version,
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
                self.store.stop(owner, id).await?;
                Err(reason.into())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn publish(&self, owner: Uuid, publish: Publish, now: DateTime<Utc>) -> Reply {
        if publish.captured_at > now + Duration::minutes(1) {
            return Err(Failure::invalid("Capture time must not be in the future"));
        }
        let mut profile = self.enrolled(owner).await?;
        if let Some(menu) = &profile.value.menu {
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
        let url = self.media.publish(profile.value.id, publish.id).await?;
        let menu = Menu {
            id: publish.id,
            captured_at: publish.captured_at,
            published_at: now,
            url,
        };
        profile.value.menu = Some(menu.clone());
        self.store
            .put_profile(owner, Some(profile.version), Record::new(profile.value))
            .await?;
        Ok(ok(menu))
    }
}
