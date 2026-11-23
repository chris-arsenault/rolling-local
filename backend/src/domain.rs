use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PUBLICATION_INTERVAL: Duration = Duration::minutes(10);
const FIX_FRESHNESS: Duration = Duration::minutes(20);
const MAX_NEW_FIX_AGE: Duration = Duration::minutes(2);
const CLOCK_SKEW: Duration = Duration::seconds(30);
const EARTH_RADIUS_METERS: f64 = 6_371_008.8;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

impl Position {
    pub fn valid(self) -> bool {
        (-90.0..=90.0).contains(&self.latitude) && (-180.0..=180.0).contains(&self.longitude)
    }

    pub fn meters_from(self, center: Self) -> f64 {
        let latitude = self.latitude.to_radians();
        let center_latitude = center.latitude.to_radians();
        let longitude_difference = (self.longitude - center.longitude).to_radians();
        let chord = ((latitude - center_latitude) / 2.0).sin().powi(2)
            + latitude.cos() * center_latitude.cos() * (longitude_difference / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_METERS * chord.clamp(0.0, 1.0).sqrt().asin()
    }

    fn rounded(self) -> Self {
        Self {
            latitude: (self.latitude * 1000.0).round() / 1000.0,
            longitude: (self.longitude * 1000.0).round() / 1000.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Circle {
    pub center: Position,
    pub radius_meters: f64,
}

impl Circle {
    fn valid(self) -> bool {
        self.center.valid() && self.radius_meters.is_finite() && self.radius_meters > 0.0
    }

    fn contains(self, fix: Fix) -> bool {
        fix.position.meters_from(self.center) + fix.accuracy_meters <= self.radius_meters
            && fix.position.rounded().meters_from(self.center) <= self.radius_meters
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventArea {
    pub area: Circle,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Hours {
    pub weekdays: Vec<u8>,
    pub opens: NaiveTime,
    pub closes: NaiveTime,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub name: String,
    pub cuisine: String,
    #[serde(flatten)]
    pub policy: OperatingPolicy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatingPolicy {
    pub timezone: Tz,
    pub hours: Hours,
    pub area: Circle,
    pub event: Option<EventArea>,
}

fn local_boundary(zone: Tz, date: NaiveDate, hour: NaiveTime) -> Option<DateTime<Utc>> {
    let time = date.and_time(hour);
    (0..=180).find_map(|minute| {
        // A repeated closing time must not extend a session through the clock change.
        zone.from_local_datetime(&(time + Duration::minutes(minute)))
            .earliest()
            .map(|time| time.with_timezone(&Utc))
    })
}

impl Hours {
    fn valid(&self) -> bool {
        !self.weekdays.is_empty()
            && self.weekdays.iter().all(|day| (1..=7).contains(day))
            && self.opens != self.closes
    }

    pub fn closes_at(&self, zone: Tz, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let today = now.with_timezone(&zone).date_naive();
        [today, today.pred_opt()?]
            .into_iter()
            .filter_map(|day| self.window(zone, day))
            .find_map(|(open, close)| (open <= now && now < close).then_some(close))
    }

    fn window(&self, zone: Tz, day: NaiveDate) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let weekday = day.weekday().number_from_monday() as u8;
        if !self.weekdays.contains(&weekday) {
            return None;
        }
        let end = if self.closes > self.opens {
            day
        } else {
            day.succ_opt()?
        };
        Some((
            local_boundary(zone, day, self.opens)?,
            local_boundary(zone, end, self.closes)?,
        ))
    }
}

impl Settings {
    pub fn valid(&self, now: DateTime<Utc>) -> bool {
        valid_listing_field(&self.name)
            && valid_listing_field(&self.cuisine)
            && self.policy.valid(now)
    }
}

fn valid_listing_field(value: &str) -> bool {
    !value.trim().is_empty() && value.trim().chars().count() <= 100
}

impl OperatingPolicy {
    fn valid(&self, now: DateTime<Utc>) -> bool {
        self.hours.valid()
            && self.area.valid()
            && self
                .event
                .as_ref()
                .is_none_or(|event| event.area.valid() && event.expires_at > now)
    }

    fn effective_area(&self, now: DateTime<Utc>) -> Circle {
        self.event
            .as_ref()
            .filter(|event| event.expires_at > now)
            .map_or(self.area, |event| event.area)
    }

    fn location_expires_at(&self, fix: Fix, now: DateTime<Utc>) -> DateTime<Utc> {
        let freshness = now.min(fix.recorded_at) + FIX_FRESHNESS;
        match self.event.as_ref().filter(|event| event.expires_at > now) {
            Some(event) if !self.area.contains(fix) => freshness.min(event.expires_at),
            _ => freshness,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub settings: Settings,
    pub menu: Option<Menu>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Listing {
    pub name: String,
    pub cuisine: String,
}

impl Listing {
    pub fn normalize(mut self) -> Option<Self> {
        if !valid_listing_field(&self.name) || !valid_listing_field(&self.cuisine) {
            return None;
        }
        self.name = self.name.trim().into();
        self.cuisine = self.cuisine.trim().into();
        Some(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TruckStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub by: String,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Truck {
    pub id: Uuid,
    #[serde(flatten)]
    pub listing: Listing,
    pub status: TruckStatus,
    pub operator_subject: Option<String>,
    pub review: Option<Review>,
    pub policy_id: Uuid,
    pub policy: Option<OperatingPolicy>,
    pub menu: Option<Menu>,
}

impl Truck {
    pub fn new(listing: Listing, operator_subject: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            listing,
            status: TruckStatus::Pending,
            operator_subject,
            review: None,
            policy_id: Uuid::new_v4(),
            policy: None,
            menu: None,
        }
    }

    pub fn from_legacy(profile: Profile) -> Self {
        Self {
            id: profile.id,
            listing: Listing {
                name: profile.settings.name,
                cuisine: profile.settings.cuisine,
            },
            status: TruckStatus::Approved,
            operator_subject: None,
            review: None,
            policy_id: profile.policy_id,
            policy: Some(profile.settings.policy),
            menu: profile.menu,
        }
    }

    pub fn operator_profile(&self) -> Option<Profile> {
        if self.status != TruckStatus::Approved || self.operator_subject.is_none() {
            return None;
        }
        Some(Profile {
            id: self.id,
            policy_id: self.policy_id,
            settings: Settings {
                name: self.listing.name.clone(),
                cuisine: self.listing.cuisine.clone(),
                policy: self.policy.clone()?,
            },
            menu: self.menu.clone(),
        })
    }

    pub fn update_settings(&mut self, mut settings: Settings) {
        settings.policy.hours.weekdays.sort_unstable();
        settings.policy.hours.weekdays.dedup();
        if self.policy.as_ref() != Some(&settings.policy) {
            self.policy_id = Uuid::new_v4();
        }
        self.listing = Listing {
            name: settings.name.trim().into(),
            cuisine: settings.cuisine.trim().into(),
        };
        self.policy = Some(settings.policy);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    pub id: Uuid,
    pub captured_at: DateTime<Utc>,
    pub published_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    pub position: Position,
    pub accuracy_meters: f64,
    pub recorded_at: DateTime<Utc>,
}

impl Fix {
    fn fresh(self, now: DateTime<Utc>) -> bool {
        self.recorded_at <= now + CLOCK_SKEW && self.recorded_at >= now - MAX_NEW_FIX_AGE
    }

    fn valid(self) -> bool {
        self.position.valid() && self.accuracy_meters.is_finite() && self.accuracy_meters >= 0.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicLocation {
    pub position: Position,
    pub recorded_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub closes_at: DateTime<Utc>,
    pub published_at: DateTime<Utc>,
    pub location: PublicLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum Rejection {
    #[error("Sharing is outside the configured hours")]
    Closed,
    #[error("The phone is outside the operating circle")]
    Outside,
    #[error("A current phone fix is required")]
    StaleFix,
    #[error("The phone fix is invalid")]
    InvalidFix,
    #[error("The sharing session has ended")]
    Ended,
}

impl Session {
    pub fn begin(
        profile: &Profile,
        id: Uuid,
        fix: Fix,
        now: DateTime<Utc>,
    ) -> Result<Self, Rejection> {
        let policy = &profile.settings.policy;
        let closes_at = policy
            .hours
            .closes_at(policy.timezone, now)
            .ok_or(Rejection::Closed)?;
        let location = Self::location(policy, closes_at, fix, now)?;
        Ok(Self {
            id,
            policy_id: profile.policy_id,
            closes_at,
            published_at: now,
            location,
        })
    }

    pub fn active(&self, profile: &Profile, now: DateTime<Utc>) -> bool {
        self.policy_id == profile.policy_id && now < self.location.expires_at
    }

    pub fn update(
        &mut self,
        profile: &Profile,
        fix: Fix,
        now: DateTime<Utc>,
    ) -> Result<bool, Rejection> {
        if !self.active(profile, now) {
            return Err(Rejection::Ended);
        }
        if fix.recorded_at <= self.location.recorded_at {
            return Ok(false);
        }
        let next = Self::location(&profile.settings.policy, self.closes_at, fix, now)?;
        if now - self.published_at < PUBLICATION_INTERVAL {
            return Ok(false);
        }
        self.location = next;
        self.published_at = now;
        Ok(true)
    }

    fn location(
        policy: &OperatingPolicy,
        closes_at: DateTime<Utc>,
        fix: Fix,
        now: DateTime<Utc>,
    ) -> Result<PublicLocation, Rejection> {
        if now >= closes_at || policy.hours.closes_at(policy.timezone, now).is_none() {
            return Err(Rejection::Closed);
        }
        if !fix.fresh(now) {
            return Err(Rejection::StaleFix);
        }
        if !fix.valid() {
            return Err(Rejection::InvalidFix);
        }
        if !policy.effective_area(now).contains(fix) {
            return Err(Rejection::Outside);
        }
        Ok(PublicLocation {
            position: fix.position.rounded(),
            recorded_at: fix.recorded_at,
            expires_at: policy.location_expires_at(fix, now).min(closes_at),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sharing {
    id: Uuid,
    published_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    closes_at: DateTime<Utc>,
}

impl From<&Session> for Sharing {
    fn from(session: &Session) -> Self {
        Self {
            id: session.id,
            published_at: session.published_at,
            expires_at: session.location.expires_at,
            closes_at: session.closes_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicTruck {
    id: Uuid,
    name: String,
    cuisine: String,
    menu: Option<Menu>,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<PublicLocation>,
}

impl PublicTruck {
    pub fn from_truck(truck: Truck, session: Option<&Session>, now: DateTime<Utc>) -> Option<Self> {
        if truck.status != TruckStatus::Approved {
            return None;
        }
        let location = session.and_then(|session| {
            let profile = truck.operator_profile()?;
            session
                .active(&profile, now)
                .then(|| session.location.clone())
        });
        Some(Self {
            id: truck.id,
            name: truck.listing.name,
            cuisine: truck.listing.cuisine,
            menu: truck.menu,
            active: location.is_some(),
            location,
        })
    }
}
