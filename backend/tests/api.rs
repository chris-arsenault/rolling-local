use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use lambda_http::{
    Body,
    http::{Method, Request, Response, StatusCode},
};
use rolling_local_api::{
    Api, OperatorConfig, Surface,
    domain::{Fix, Position, Profile, PublicTruck, Session, Truck},
    media::{Media, MediaError, UploadTicket},
    menus::Extraction,
    storage::{Record, Store, StoreError},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Default)]
struct Records {
    extractions: HashMap<(Uuid, Uuid), Record<Extraction>>,
    trucks: HashMap<Uuid, Record<Truck>>,
    operators: HashMap<String, Uuid>,
    sessions: HashMap<Uuid, Record<Session>>,
    starts: HashSet<(Uuid, Uuid)>,
}

#[derive(Default)]
struct Memory(Mutex<Records>);

#[async_trait]
impl Store for Memory {
    async fn extraction(
        &self,
        truck: Uuid,
        menu: Uuid,
    ) -> Result<Option<Record<Extraction>>, StoreError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .extractions
            .get(&(truck, menu))
            .cloned())
    }
    async fn publish_menu(
        &self,
        previous: Uuid,
        truck: Record<Truck>,
        extraction: Record<Extraction>,
    ) -> Result<(), StoreError> {
        let mut db = self.0.lock().unwrap();
        let key = (truck.value.id, extraction.value.menu_id);
        if db.trucks.get(&truck.value.id).map(|r| r.version) != Some(previous)
            || db.extractions.contains_key(&key)
        {
            return Err(StoreError::Conflict);
        }
        db.trucks.insert(truck.value.id, truck);
        db.extractions.insert(key, extraction);
        Ok(())
    }
    async fn write_extraction(
        &self,
        record: &Record<Extraction>,
        previous: Option<Uuid>,
        truck_version: Option<Uuid>,
    ) -> Result<(), StoreError> {
        let mut db = self.0.lock().unwrap();
        let key = (record.value.truck_id, record.value.menu_id);
        if db.extractions.get(&key).map(|r| r.version) != previous
            || truck_version.is_some_and(|v| {
                db.trucks.get(&record.value.truck_id).map(|r| r.version) != Some(v)
            })
        {
            return Err(StoreError::Conflict);
        }
        db.extractions.insert(key, record.clone());
        Ok(())
    }
    async fn truck(&self, id: Uuid) -> Result<Option<Record<Truck>>, StoreError> {
        Ok(self.0.lock().unwrap().trucks.get(&id).cloned())
    }

    async fn owned_truck(&self, owner: &str) -> Result<Option<Record<Truck>>, StoreError> {
        let db = self.0.lock().unwrap();
        Ok(db
            .operators
            .get(owner)
            .and_then(|id| db.trucks.get(id))
            .cloned())
    }

    async fn session(&self, owner: Uuid) -> Result<Option<Record<Session>>, StoreError> {
        Ok(self.0.lock().unwrap().sessions.get(&owner).cloned())
    }

    async fn put_truck(
        &self,
        previous: Option<Uuid>,
        truck: Record<Truck>,
        link_owner: bool,
    ) -> Result<(), StoreError> {
        let mut db = self.0.lock().unwrap();
        let id = truck.value.id;
        if db.trucks.get(&id).map(|entry| entry.version) != previous {
            return Err(StoreError::Conflict);
        }
        if link_owner {
            let subject = truck.value.operator_subject.clone().unwrap();
            if db.operators.contains_key(&subject) {
                return Err(StoreError::Conflict);
            }
            db.operators.insert(subject, id);
        }
        db.trucks.insert(id, truck);
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
        let mut db = self.0.lock().unwrap();
        if db.trucks.get(&owner).map(|entry| entry.version) != Some(profile_version)
            || db.sessions.get(&owner).map(|entry| entry.version) != previous
            || (start_command && !db.starts.insert((owner, session.value.id)))
        {
            return Err(StoreError::Conflict);
        }
        db.sessions.insert(owner, session);
        Ok(())
    }

    async fn stop(&self, owner: Uuid, id: Uuid) -> Result<(), StoreError> {
        let mut db = self.0.lock().unwrap();
        if db
            .sessions
            .get(&owner)
            .is_some_and(|entry| entry.value.id == id)
        {
            db.sessions.remove(&owner);
        }
        Ok(())
    }

    async fn directory(&self) -> Result<Vec<(Record<Truck>, Option<Session>)>, StoreError> {
        let db = self.0.lock().unwrap();
        Ok(db
            .trucks
            .iter()
            .map(|(id, truck)| {
                (
                    truck.clone(),
                    db.sessions.get(id).map(|record| record.value.clone()),
                )
            })
            .collect())
    }
}

#[derive(Default)]
struct MockMedia(Mutex<HashMap<Uuid, Uuid>>);

#[async_trait]
impl Media for MockMedia {
    async fn upload(&self, truck: Uuid, bytes: i64) -> Result<UploadTicket, MediaError> {
        assert!(bytes > 0);
        let id = Uuid::new_v4();
        self.0.lock().unwrap().insert(id, truck);
        Ok(UploadTicket {
            id,
            url: format!("https://s3.test/{id}.jpg"),
        })
    }

    async fn publish(&self, truck: Uuid, id: Uuid) -> Result<String, MediaError> {
        if self.0.lock().unwrap().get(&id) != Some(&truck) {
            return Err(MediaError::Invalid);
        }
        Ok(format!(
            "https://rolling-local.test/data/menus/{truck}/{id}.jpg"
        ))
    }
}

fn apis() -> (Api, Api) {
    with_store(Arc::new(Memory::default()))
}

fn with_store(store: Arc<Memory>) -> (Api, Api) {
    let media = Arc::new(MockMedia::default());
    let config = OperatorConfig {
        issuer: "https://signin.test/".into(),
        audience: "https://api.rolling-local.test".into(),
        connection: "ahara".into(),
        web_client_id: "operator-client".into(),
        ios_client_id: "ios-client".into(),
        android_client_id: "android-client".into(),
        redirect_uri: "rollinglocal://oauth".into(),
        public_site: "https://rolling-local.test".into(),
        admin_subject: Some("auth0|owner".into()),
    };
    (
        Api::new(
            store.clone(),
            media.clone(),
            config.clone(),
            Surface::Operator,
        ),
        Api::new(store, media, config, Surface::Public),
    )
}

struct SimulatedReader(&'static str);

#[async_trait]
impl rolling_local_api::extraction_worker::MenuReader for SimulatedReader {
    async fn read(
        &self,
        photo: &str,
    ) -> Result<Vec<rolling_local_api::menus::Fields>, &'static str> {
        assert!(photo.starts_with("data/menus/") && photo.ends_with(".jpg"));
        match self.0 {
            "fail" => Err("provider_unavailable"),
            "unused" => panic!("A completed or superseded menu triggered inference"),
            tag => Ok(vec![serde_json::from_value(json!({
                "name":"Tuna roll","description":null,"priceText":"Small $6 / Large $9","tagIds":[tag]
            })).unwrap()]),
        }
    }
}

async fn publish_photo(api: &Api, token: &str) -> (Uuid, Uuid) {
    let ticket = request(
        api,
        Method::POST,
        "/operator/menu/upload",
        Some(token),
        json!({"bytes":1200}),
        now(),
    )
    .await;
    let photo = request(
        api,
        Method::POST,
        "/operator/menu/publish",
        Some(token),
        json!({"id":value(&ticket)["id"],"capturedAt":now()}),
        now(),
    )
    .await;
    assert_eq!(photo.status(), StatusCode::OK);
    let registration = request(
        api,
        Method::GET,
        "/operator",
        Some(token),
        Value::Null,
        now(),
    )
    .await;
    (
        serde_json::from_value(value(&registration)["registration"]["id"].clone()).unwrap(),
        serde_json::from_value(value(&photo)["id"].clone()).unwrap(),
    )
}

async fn catalog(api: &Api) -> Value {
    let reply = request(
        api,
        Method::GET,
        "/public/menu-catalog",
        None,
        Value::Null,
        now(),
    )
    .await;
    assert_eq!(reply.status(), StatusCode::OK);
    assert_eq!(reply.headers()["cache-control"], "no-store");
    value(&reply)
}

#[tokio::test]
async fn photo_to_worker_catalog_and_operator_correction() {
    use rolling_local_api::extraction_worker::process;
    let db = Arc::new(Memory::default());
    let (operator, public) = with_store(db.clone());
    let owner = auth(Uuid::new_v4(), "ios-client");
    let other = auth(Uuid::new_v4(), "android-client");
    enroll(&operator, &owner, settings("Night Market")).await;
    enroll(&operator, &other, settings("Other truck")).await;
    let (truck, id) = publish_photo(&operator, &owner).await;
    let photo = catalog(&public).await["menus"][0].clone();
    assert_eq!(photo["status"], "queued");
    assert_eq!(photo["items"], json!([]));
    for failure in ["fail", "unsupported-tag"] {
        assert!(
            process(db.as_ref(), &SimulatedReader(failure), truck, id, id)
                .await
                .is_err()
        );
        let failed = catalog(&public).await;
        assert_eq!(failed["menus"][0]["status"], "failed");
        assert_eq!(failed["menus"][0]["url"], photo["url"]);
        assert_eq!(failed["menus"][0]["items"], json!([]));
    }
    let failed = db.extraction(truck, id).await.unwrap().unwrap();
    let retry = request(
        &operator,
        Method::POST,
        "/operator/menu/extraction/retry",
        Some(&owner),
        json!({"menuId":id, "revision":failed.version}),
        now(),
    )
    .await;
    assert_eq!(retry.status(), StatusCode::OK);
    let generation = db
        .extraction(truck, id)
        .await
        .unwrap()
        .unwrap()
        .value
        .job_id;
    process(db.as_ref(), &SimulatedReader("unused"), truck, id, id)
        .await
        .unwrap();
    process(
        db.as_ref(),
        &SimulatedReader("sushi"),
        truck,
        id,
        generation,
    )
    .await
    .unwrap();
    let ready = catalog(&public).await;
    let parsed = &ready["menus"][0]["items"][0];
    assert_eq!(parsed["priceText"], "Small $6 / Large $9");
    assert_eq!(parsed["description"], Value::Null);
    assert_eq!(parsed["tagIds"], json!(["sushi"]));
    assert_eq!(ready["tags"].as_array().unwrap().len(), 25);
    let path = "/operator/menu/extraction";
    let current = value(
        &request(
            &operator,
            Method::GET,
            path,
            Some(&owner),
            Value::Null,
            now(),
        )
        .await,
    );
    let mut edit = json!({"menuId":id,"revision":current["extraction"]["revision"],"items":current["extraction"]["items"]});
    edit["items"][0]["priceText"] = json!("$11");
    assert_eq!(
        request(
            &operator,
            Method::PUT,
            path,
            Some(&other),
            edit.clone(),
            now()
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &operator,
            Method::PUT,
            path,
            Some(&owner),
            edit.clone(),
            now()
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&operator, Method::PUT, path, Some(&owner), edit, now())
            .await
            .status(),
        StatusCode::CONFLICT
    );
    process(
        db.as_ref(),
        &SimulatedReader("unused"),
        truck,
        id,
        generation,
    )
    .await
    .unwrap();
    let corrected = catalog(&public).await;
    assert_eq!(corrected["menus"][0]["items"][0]["priceText"], "$11");
    let listings = trucks(&public, now()).await;
    assert!(
        listings
            .iter()
            .all(|truck| truck.get("items").is_none() && truck.get("location").is_none())
    );
    // The consumer contract test searches these responses through its normal load functions.
    println!(
        "DISCOVERY_SAMPLE {}",
        json!({"catalog":corrected,"directory":{"trucks":listings}})
    );
    let (_, newer) = publish_photo(&operator, &owner).await;
    assert_ne!(newer, id);
    assert_eq!(catalog(&public).await["menus"][0]["menuId"], json!(newer));
    assert_eq!(catalog(&public).await["menus"][0]["items"], json!([]));
    let (_, latest) = publish_photo(&operator, &owner).await;
    process(db.as_ref(), &SimulatedReader("unused"), truck, newer, newer)
        .await
        .unwrap();
    assert_eq!(catalog(&public).await["menus"][0]["menuId"], json!(latest));
}

#[tokio::test]
async fn catalog_pages_include_only_current_approved_menus_and_keep_legacy_photos() {
    use lambda_http::RequestExt;
    use rolling_local_api::{
        domain::{Listing, Menu, TruckStatus},
        menus::{Fields, MenuItem, Status},
    };
    let db = Arc::new(Memory::default());
    let (_, public) = with_store(db.clone());
    let mut visible = Vec::new();
    for index in 1..=6 {
        let id = Uuid::from_u128(index);
        let mut truck = Truck::new(
            Listing {
                name: format!("Lunch {index}"),
                cuisine: "Street food".into(),
            },
            None,
        );
        truck.id = id;
        truck.status = match index {
            5 => TruckStatus::Pending,
            6 => TruckStatus::Rejected,
            _ => TruckStatus::Approved,
        };
        let menu_id = Uuid::new_v4();
        truck.menu = Some(Menu {
            id: menu_id,
            url: format!("https://test.test/{menu_id}"),
            captured_at: now(),
            published_at: now(),
        });
        db.put_truck(None, Record::new(truck), false).await.unwrap();
        if index <= 4 {
            visible.push(id.to_string());
        }
        if index == 1 {
            continue;
        }
        let mut row = Extraction::queued(id, menu_id);
        row.status = Status::Ready;
        row.items = (0..32)
            .map(|position| MenuItem {
                id: Uuid::new_v4(),
                position,
                fields: Fields {
                    name: "Café   spécial".into(),
                    description: Some("a".repeat(4000)),
                    price_text: None,
                    tag_ids: vec![],
                },
            })
            .collect();
        db.write_extraction(&Record::new(row), None, None)
            .await
            .unwrap();
    }
    let mut page = catalog(&public).await;
    assert!(page["menus"][0]["status"].is_null());
    assert_eq!(page["menus"][0]["items"], json!([]));
    assert_eq!(page["menus"][1]["items"][0]["searchName"], "cafe special");
    assert!(page["next"].is_string());
    let mut seen = Vec::new();
    loop {
        seen.extend(
            page["menus"]
                .as_array()
                .unwrap()
                .iter()
                .map(|menu| menu["truckId"].as_str().unwrap().to_owned()),
        );
        let Some(after) = page["next"].as_str() else {
            break;
        };
        let params = HashMap::from([("after".to_string(), after.to_string())]);
        let req = Request::builder()
            .method(Method::GET)
            .uri("/public/menu-catalog")
            .body(Body::Empty)
            .unwrap()
            .with_query_string_parameters(params);
        page = value(&public.handle(req, now()).await);
        assert!(seen.len() <= visible.len());
    }
    assert_eq!(seen, visible);
}

struct PausedReader {
    started: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

#[async_trait]
impl rolling_local_api::extraction_worker::MenuReader for PausedReader {
    async fn read(
        &self,
        photo: &str,
    ) -> Result<Vec<rolling_local_api::menus::Fields>, &'static str> {
        self.started.notify_one();
        self.resume.notified().await;
        SimulatedReader("sushi").read(photo).await
    }
}

#[tokio::test]
async fn a_worker_finishing_after_a_correction_cannot_overwrite_it() {
    let db = Arc::new(Memory::default());
    let (operator, public) = with_store(db.clone());
    let token = auth(Uuid::new_v4(), "ios-client");
    enroll(&operator, &token, settings("Lunch")).await;
    let (truck, id) = publish_photo(&operator, &token).await;
    let reader = PausedReader {
        started: Default::default(),
        resume: Default::default(),
    };
    let job = rolling_local_api::extraction_worker::process(db.as_ref(), &reader, truck, id, id);
    let correct = async {
        reader.started.notified().await;
        let path = "/operator/menu/extraction";
        let current = value(
            &request(
                &operator,
                Method::GET,
                path,
                Some(&token),
                Value::Null,
                now(),
            )
            .await,
        );
        assert_eq!(current["extraction"]["status"], "processing");
        let edit = json!({"menuId":id,"revision":current["extraction"]["revision"],"items":[]});
        let reply = request(&operator, Method::PUT, path, Some(&token), edit, now()).await;
        assert_eq!(reply.status(), StatusCode::OK);
        reader.resume.notify_one();
    };
    let (result, ()) = tokio::join!(job, correct);
    assert!(result.is_err());
    rolling_local_api::extraction_worker::process(
        db.as_ref(),
        &SimulatedReader("unused"),
        truck,
        id,
        id,
    )
    .await
    .unwrap();
    assert_eq!(catalog(&public).await["menus"][0]["items"], json!([]));
}

fn auth(owner: Uuid, client: &str) -> String {
    auth_with_audience(
        &format!("auth0|{owner}"),
        client,
        "https://api.rolling-local.test",
    )
}

fn admin_token() -> String {
    auth_with_audience(
        "auth0|owner",
        "operator-client",
        "https://api.rolling-local.test",
    )
}

fn auth_with_audience(owner: &str, client: &str, audience: &str) -> String {
    let claims =
        json!({ "sub": owner, "iss": "https://signin.test/", "azp": client, "aud": [audience] });
    format!(
        "Bearer header.{}.signature",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

fn now() -> DateTime<Utc> {
    "2026-11-17T18:30:00Z".parse().unwrap()
}

fn settings(name: &str) -> Value {
    json!({
        "name": name, "cuisine": "Tacos", "timezone": "America/Phoenix",
        "hours": { "weekdays": [2, 3, 4, 5, 6], "opens": "11:00:00", "closes": "12:00:00" },
        "area": { "center": { "latitude": 34.54, "longitude": -112.46 }, "radiusMeters": 7000 },
        "event": null
    })
}

fn fix(latitude: f64, time: DateTime<Utc>) -> Fix {
    Fix {
        position: Position {
            latitude,
            longitude: -112.459814,
        },
        accuracy_meters: 10.0,
        recorded_at: time,
    }
}

async fn request(
    api: &Api,
    method: Method,
    path: &str,
    token: Option<&str>,
    data: Value,
    time: DateTime<Utc>,
) -> Response<Body> {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        req = req.header("authorization", token);
    }
    api.handle(req.body(Body::Text(data.to_string())).unwrap(), time)
        .await
}

fn value(response: &Response<Body>) -> Value {
    serde_json::from_slice(response.body().as_ref()).unwrap()
}

async fn trucks(public: &Api, time: DateTime<Utc>) -> Vec<Value> {
    let result = request(
        public,
        Method::GET,
        "/public/trucks",
        None,
        Value::Null,
        time,
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()["cache-control"], "no-store");
    value(&result)["trucks"].as_array().unwrap().clone()
}

async fn enroll(operator: &Api, token: &str, settings: Value) {
    let registration = request(
        operator,
        Method::POST,
        "/operator/registration",
        Some(token),
        json!({"name": settings["name"], "cuisine": settings["cuisine"]}),
        now(),
    )
    .await;
    assert_eq!(registration.status(), StatusCode::OK);
    let id = value(&registration)["registration"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    decide(operator, &id, "accept").await;
    let result = request(
        operator,
        Method::PUT,
        "/operator/settings",
        Some(token),
        settings,
        now(),
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK, "{:?}", value(&result));
}

async fn decide(api: &Api, id: &str, decision: &str) {
    let admin = admin_token();
    let response = request(
        api,
        Method::GET,
        "/admin/trucks",
        Some(&admin),
        Value::Null,
        now(),
    )
    .await;
    let trucks = value(&response);
    let entry = trucks["trucks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|truck| truck["id"] == id)
        .unwrap();
    let response = request(
        api,
        Method::PUT,
        &format!("/admin/trucks/{id}"),
        Some(&admin),
        json!({"version": entry["version"], "decision": decision}),
        now(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "{:?}", value(&response));
}

#[tokio::test]
async fn registration_is_private_until_owner_approval_and_manual_entries_can_publish_offline() {
    let (api, public) = apis();
    let admin = admin_token();
    let first = auth(Uuid::new_v4(), "operator-client");
    let second = auth(Uuid::new_v4(), "android-client");
    let application = json!({"name": "Copper Taco", "cuisine": "Tacos"});
    let submit = |token, data| {
        request(
            &api,
            Method::POST,
            "/operator/registration",
            Some(token),
            data,
            now(),
        )
    };
    let original = submit(&first, application.clone()).await;
    let original = value(&original)["registration"].clone();
    assert_eq!(original["status"], "pending");
    let retry = submit(&first, application).await;
    assert_eq!(value(&retry)["registration"]["id"], original["id"]);
    let declined = submit(&second, json!({"name": "Second Truck", "cuisine": "BBQ"})).await;
    let declined = value(&declined)["registration"].clone();
    assert!(trucks(&public, now()).await.is_empty());

    let unauthorized = request(
        &api,
        Method::GET,
        "/admin/trucks",
        Some(&first),
        Value::Null,
        now(),
    )
    .await;
    assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);
    let wrong_audience =
        auth_with_audience("auth0|owner", "operator-client", "https://another-app.test");
    let unauthorized = request(
        &api,
        Method::GET,
        "/admin/trucks",
        Some(&wrong_audience),
        Value::Null,
        now(),
    )
    .await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let upload = request(
        &api,
        Method::POST,
        "/operator/menu/upload",
        Some(&first),
        json!({"bytes": 320}),
        now(),
    )
    .await;
    assert_eq!(upload.status(), StatusCode::FORBIDDEN);

    decide(&api, original["id"].as_str().unwrap(), "accept").await;
    decide(&api, declined["id"].as_str().unwrap(), "reject").await;
    let declined_view = request(
        &api,
        Method::GET,
        "/operator",
        Some(&second),
        Value::Null,
        now(),
    )
    .await;
    assert_eq!(value(&declined_view)["registration"]["status"], "rejected");

    let manual_id = Uuid::new_v4();
    let data =
        json!({"id": manual_id, "name": "Saturday Bakery", "cuisine": "Bakery", "publish": true});
    for _ in 0..2 {
        let created = request(
            &api,
            Method::POST,
            "/admin/trucks",
            Some(&admin),
            data.clone(),
            now(),
        )
        .await;
        assert_eq!(created.status(), StatusCode::OK);
        assert_eq!(value(&created)["status"], "approved");
        assert!(value(&created)["operatorSubject"].is_null());
    }

    let entries = trucks(&public, now()).await;
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry["id"] == original["id"]));
    assert!(
        entries
            .iter()
            .any(|entry| entry["id"] == manual_id.to_string())
    );
    for entry in entries {
        assert_eq!(entry["active"], false);
        assert!(entry.get("location").is_none());
        assert!(entry.get("operatorSubject").is_none());
        assert!(entry.get("review").is_none());
    }
}

#[tokio::test]
async fn preflight_accepts_only_the_product_site_and_reveals_no_admin_data() {
    let (_, public) = apis();
    for (origin, status) in [
        ("https://rolling-local.test", StatusCode::NO_CONTENT),
        ("https://other.test", StatusCode::FORBIDDEN),
    ] {
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/admin/trucks")
            .header("origin", origin)
            .body(Body::Empty)
            .unwrap();
        let reply = public.handle(request, now()).await;
        assert_eq!(reply.status(), status);
        assert_eq!(
            reply.headers().contains_key("access-control-allow-origin"),
            status == StatusCode::NO_CONTENT
        );
    }
}

#[test]
fn legacy_profiles_keep_listing_and_menu_but_never_reuse_an_old_session() {
    let truck_id = Uuid::new_v4();
    let menu_url = "https://rolling-local.test/data/menus/old.jpg";
    let profile: Profile = serde_json::from_value(json!({
        "id": truck_id,
        "policyId": Uuid::new_v4(),
        "settings": settings("Legacy Truck"),
        "menu": {"id": Uuid::new_v4(), "capturedAt": now(), "publishedAt": now(), "url": menu_url},
    }))
    .unwrap();
    let session = Session::begin(&profile, Uuid::new_v4(), fix(34.54, now()), now()).unwrap();
    let truck = Truck::from_legacy(profile);
    assert!(truck.operator_subject.is_none());
    let public =
        serde_json::to_value(PublicTruck::from_truck(truck, Some(&session), now())).unwrap();
    assert_eq!(public["id"], truck_id.to_string());
    assert_eq!(public["menu"]["url"], menu_url);
    assert_eq!(public["active"], false);
    assert!(public.get("location").is_none());
}

#[tokio::test]
async fn broadcasts_follow_moving_phone_at_cadence_and_stop_when_owner_leaves_circle() {
    let (operator, public) = apis();
    let token = auth(Uuid::new_v4(), "operator-client");
    enroll(&operator, &token, settings("Copper Truck")).await;
    enroll(
        &operator,
        &auth(Uuid::new_v4(), "operator-client"),
        settings("Offline Truck"),
    )
    .await;
    let id = Uuid::new_v4();
    let first = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        json!({ "id": id, "fix": fix(34.541111, now()) }),
        now(),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let published = trucks(&public, now()).await;
    let active = published
        .iter()
        .find(|t| t["name"] == "Copper Truck")
        .unwrap();
    let offline = published
        .iter()
        .find(|t| t["name"] == "Offline Truck")
        .unwrap();
    assert_eq!(active["location"]["position"]["latitude"], 34.541111);
    assert_eq!(offline["active"], false);
    assert!(offline.get("location").is_none());

    let path = format!("/operator/sessions/{id}/fix");
    let early = now() + Duration::minutes(4);
    let moved = request(
        &operator,
        Method::PUT,
        &path,
        Some(&token),
        json!(fix(34.5513, early)),
        early,
    )
    .await;
    assert_eq!(moved.status(), StatusCode::OK);
    assert_eq!(value(&moved), value(&first));
    let due = now() + Duration::minutes(10);
    let moved = request(
        &operator,
        Method::PUT,
        &path,
        Some(&token),
        json!(fix(34.5513, due)),
        due,
    )
    .await;
    assert_eq!(moved.status(), StatusCode::OK);
    assert_ne!(value(&moved)["publishedAt"], value(&first)["publishedAt"]);
    let moved = trucks(&public, due).await;
    let active = moved.iter().find(|t| t["name"] == "Copper Truck").unwrap();
    assert_eq!(active["location"]["position"]["latitude"], 34.5513);

    let outside = due + Duration::seconds(20);
    let outcome = request(
        &operator,
        Method::PUT,
        &path,
        Some(&token),
        json!(fix(35.1, outside)),
        outside,
    )
    .await;
    assert_eq!(outcome.status(), StatusCode::CONFLICT);
    assert!(
        trucks(&public, outside)
            .await
            .iter()
            .all(|t| t.get("location").is_none())
    );
}

#[tokio::test]
async fn closing_time_and_event_override_expiry_remove_public_location() {
    let (operator, public) = apis();
    let token = auth(Uuid::new_v4(), "operator-client");
    let mut config = settings("Event Truck");
    config["hours"]["closes"] = json!("11:45:00");
    let override_end = now() + Duration::minutes(5);
    config["event"] = json!({
        "area": { "center": { "latitude": 35.1, "longitude": -112.46 }, "radiusMeters": 9000 },
        "expiresAt": override_end
    });
    enroll(&operator, &token, config).await;
    let start = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        json!({ "id": Uuid::new_v4(), "fix": fix(35.1, now()) }),
        now(),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(trucks(&public, now()).await[0]["active"], true);
    assert!(
        trucks(&public, override_end).await[0]
            .get("location")
            .is_none()
    );

    let start = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        json!({ "id": Uuid::new_v4(), "fix": fix(34.54, override_end) }),
        override_end,
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(trucks(&public, override_end).await[0]["active"], true);
    let closed = now() + Duration::minutes(15);
    assert!(trucks(&public, closed).await[0].get("location").is_none());
    let outcome = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        json!({ "id": Uuid::new_v4(), "fix": fix(34.54, closed) }),
        closed,
    )
    .await;
    assert_eq!(outcome.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn stops_are_idempotent_and_retried_starts_cannot_restart_sharing() {
    let (operator, public) = apis();
    let token = auth(Uuid::new_v4(), "operator-client");
    enroll(&operator, &token, settings("Truck")).await;
    let id = Uuid::new_v4();
    let start = json!({ "id": id, "fix": fix(34.54, now()) });
    let publish = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        start.clone(),
        now(),
    )
    .await;
    assert_eq!(publish.status(), StatusCode::OK);
    for _ in 0..2 {
        let stop = request(
            &operator,
            Method::DELETE,
            &format!("/operator/sessions/{id}"),
            Some(&token),
            Value::Null,
            now(),
        )
        .await;
        assert_eq!(stop.status(), StatusCode::OK);
    }
    let replay = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        start,
        now(),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::CONFLICT);
    assert!(trucks(&public, now()).await[0].get("location").is_none());
}

#[tokio::test]
async fn losing_phone_updates_expires_public_position_during_open_hours() {
    let (operator, public) = apis();
    let token = auth(Uuid::new_v4(), "operator-client");
    enroll(&operator, &token, settings("Truck")).await;
    let id = Uuid::new_v4();
    let started = request(
        &operator,
        Method::POST,
        "/operator/sessions",
        Some(&token),
        json!({ "id": id, "fix": fix(34.54, now()) }),
        now(),
    )
    .await;
    assert_eq!(started.status(), StatusCode::OK);
    let expires = now() + Duration::minutes(20);
    assert_eq!(
        trucks(&public, expires - Duration::seconds(1)).await[0]["active"],
        true
    );
    assert!(trucks(&public, expires).await[0].get("location").is_none());
    let later = request(
        &operator,
        Method::PUT,
        &format!("/operator/sessions/{id}/fix"),
        Some(&token),
        json!(fix(34.54, expires)),
        expires,
    )
    .await;
    assert_eq!(later.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn operators_are_scoped_to_the_product_and_only_publish_their_own_menus() {
    let (operator, public) = apis();
    let owner = Uuid::new_v4();
    let token = auth(owner, "operator-client");
    let second = auth(Uuid::new_v4(), "operator-client");
    let wrong_app = request(
        &operator,
        Method::PUT,
        "/operator/settings",
        Some(&auth(owner, "other-client")),
        settings("First"),
        now(),
    )
    .await;
    assert_eq!(wrong_app.status(), StatusCode::UNAUTHORIZED);
    enroll(&operator, &token, settings("First")).await;
    enroll(&operator, &second, settings("Second")).await;
    let upload = request(
        &operator,
        Method::POST,
        "/operator/menu/upload",
        Some(&token),
        json!({ "bytes": 1200 }),
        now(),
    )
    .await;
    assert_eq!(upload.status(), StatusCode::OK);
    let publish = json!({ "id": value(&upload)["id"], "capturedAt": now() });
    let wrong_truck = request(
        &operator,
        Method::POST,
        "/operator/menu/publish",
        Some(&second),
        publish.clone(),
        now(),
    )
    .await;
    assert_eq!(wrong_truck.status(), StatusCode::BAD_REQUEST);
    let menu = request(
        &operator,
        Method::POST,
        "/operator/menu/publish",
        Some(&token),
        publish,
        now(),
    )
    .await;
    assert_eq!(menu.status(), StatusCode::OK);
    let directory = trucks(&public, now()).await;
    let owner_entry = directory.iter().find(|t| t["name"] == "First").unwrap();
    assert_eq!(owner_entry["menu"], value(&menu));
    assert!(owner_entry.get("location").is_none());
}
