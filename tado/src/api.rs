//! The tado° REST API: homes, rooms, room states and setting a room's temperature.
//!
//! Classic tado° homes use my.tado.com/api/v2 ("zones"); tado° X homes use hops.tado.com
//! ("rooms"). tado° allows only 100 API calls a day without an Auto-Assist subscription, and
//! reports what is left in a `ratelimit` header, which is kept in [`QUOTA`].

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::{self, AuthError, HTTP};

fn my_base() -> String {
	std::env::var("TADO_API_BASE").unwrap_or_else(|_| "https://my.tado.com/api/v2".into())
}
fn hops_base() -> String {
	std::env::var("TADO_HOPS_BASE").unwrap_or_else(|_| "https://hops.tado.com".into())
}

#[derive(Debug, Clone)]
pub enum ApiError {
	SignedOut,
	/// The daily allowance is used up; holds when it refills, if tado° said.
	RateLimited(Option<Instant>),
	Http(u16),
	Network(String),
}

impl std::fmt::Display for ApiError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ApiError::SignedOut => write!(f, "Not signed in to tado°"),
			ApiError::RateLimited(_) => write!(f, "tado°'s daily request limit is used up"),
			ApiError::Http(code) => write!(f, "tado° answered with error {code}"),
			ApiError::Network(e) => write!(f, "Could not reach tado°: {e}"),
		}
	}
}

impl From<AuthError> for ApiError {
	fn from(e: AuthError) -> Self {
		match e {
			AuthError::SignedOut => ApiError::SignedOut,
			AuthError::Network(e) => ApiError::Network(e),
		}
	}
}

#[derive(Default, Clone, Copy)]
pub struct Quota {
	/// Calls left today, from the last response.
	pub remaining: Option<u32>,
	/// When the allowance refills, if it is used up.
	pub refill: Option<Instant>,
}

pub static QUOTA: LazyLock<Mutex<Quota>> = LazyLock::new(Default::default);

/// Reads `ratelimit: "perday";r=123` or `"perday";r=0;t=3600`.
pub fn parse_ratelimit(value: &str) -> Quota {
	let mut quota = Quota::default();
	for part in value.split(';') {
		let part = part.trim();
		if let Some(r) = part.strip_prefix("r=") {
			quota.remaining = r.trim().parse().ok();
		} else if let Some(t) = part.strip_prefix("t=") {
			if let Ok(secs) = t.trim().parse::<u64>() {
				quota.refill = Some(Instant::now() + Duration::from_secs(secs));
			}
		}
	}
	quota
}

async fn request(method: Method, url: &str, body: Option<&Value>) -> Result<Value, ApiError> {
	{
		let quota = *QUOTA.lock().unwrap();
		if quota.remaining == Some(0) {
			if let Some(refill) = quota.refill.filter(|r| *r > Instant::now()) {
				return Err(ApiError::RateLimited(Some(refill)));
			}
		}
	}
	for attempt in 0..2 {
		let token = auth::access_token().await?;
		let mut builder = HTTP.request(method.clone(), url).bearer_auth(token);
		if let Some(body) = body {
			builder = builder.json(body);
		}
		log::info!("tado° API: {method} {url}");
		let response = builder.send().await.map_err(|e| ApiError::Network(e.to_string()))?;
		if let Some(header) = response.headers().get("ratelimit").and_then(|v| v.to_str().ok()) {
			let quota = parse_ratelimit(header);
			log::info!("tado° calls left today: {:?}", quota.remaining);
			*QUOTA.lock().unwrap() = quota;
		}
		let status = response.status();
		if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
			auth::invalidate_access().await;
			continue;
		}
		if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
			let refill = QUOTA.lock().unwrap().refill;
			QUOTA.lock().unwrap().remaining = Some(0);
			return Err(ApiError::RateLimited(refill));
		}
		if !status.is_success() {
			return Err(ApiError::Http(status.as_u16()));
		}
		let text = response.text().await.map_err(|e| ApiError::Network(e.to_string()))?;
		return Ok(serde_json::from_str(&text).unwrap_or(Value::Null));
	}
	Err(ApiError::Http(401))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Room {
	pub id: i64,
	pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Home {
	pub id: i64,
	pub name: String,
	/// tado° X ("LINE_X") homes use a different API.
	pub x: bool,
	pub rooms: Vec<Room>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomState {
	pub current: Option<f64>,
	/// Target temperature; None when the heating is off.
	pub target: Option<f64>,
	pub humidity: Option<f64>,
	/// Heating demand 0..100.
	pub heating: Option<f64>,
	/// A manual change is active instead of the schedule.
	pub manual: bool,
}

fn num(v: &Value, path: &[&str]) -> Option<f64> {
	path.iter().try_fold(v, |v, key| v.get(key))?.as_f64()
}

fn powered(v: &Value) -> bool {
	v.pointer("/setting/power").and_then(Value::as_str) == Some("ON")
}

/// Classic zone state from `zoneStates`.
pub fn parse_zone_state(v: &Value) -> RoomState {
	RoomState {
		current: num(v, &["sensorDataPoints", "insideTemperature", "celsius"]),
		target: if powered(v) { num(v, &["setting", "temperature", "celsius"]) } else { None },
		humidity: num(v, &["sensorDataPoints", "humidity", "percentage"]),
		heating: num(v, &["activityDataPoints", "heatingPower", "percentage"]),
		manual: v.get("overlay").is_some_and(|o| !o.is_null()) || v.get("overlayType").is_some_and(|o| !o.is_null()),
	}
}

/// tado° X room state from `rooms`.
pub fn parse_room_state(v: &Value) -> RoomState {
	RoomState {
		current: num(v, &["sensorDataPoints", "insideTemperature", "value"]),
		target: if powered(v) { num(v, &["setting", "temperature", "value"]) } else { None },
		humidity: num(v, &["sensorDataPoints", "humidity", "percentage"]),
		heating: num(v, &["heatingPower", "percentage"]),
		manual: v.get("manualControlTermination").is_some_and(|o| !o.is_null()),
	}
}

/// All homes of the account with their heating rooms. Costs 1 + 2 calls per home.
pub async fn homes() -> Result<Vec<Home>, ApiError> {
	let me = request(Method::GET, &format!("{}/me", my_base()), None).await?;
	let mut homes = Vec::new();
	for home in me.get("homes").and_then(Value::as_array).into_iter().flatten() {
		let Some(id) = home.get("id").and_then(Value::as_i64) else { continue };
		let name = home.get("name").and_then(Value::as_str).unwrap_or("Home").to_owned();
		let info = request(Method::GET, &format!("{}/homes/{id}", my_base()), None).await?;
		let x = info.get("generation").and_then(Value::as_str) == Some("LINE_X");
		let rooms = if x {
			let list = request(Method::GET, &format!("{}/homes/{id}/rooms", hops_base()), None).await?;
			list.as_array()
				.into_iter()
				.flatten()
				.filter_map(|r| Some(Room { id: r.get("id")?.as_i64()?, name: r.get("name")?.as_str()?.to_owned() }))
				.collect()
		} else {
			let list = request(Method::GET, &format!("{}/homes/{id}/zones", my_base()), None).await?;
			list.as_array()
				.into_iter()
				.flatten()
				.filter(|z| z.get("type").and_then(Value::as_str) == Some("HEATING"))
				.filter_map(|z| Some(Room { id: z.get("id")?.as_i64()?, name: z.get("name")?.as_str()?.to_owned() }))
				.collect()
		};
		homes.push(Home { id, name, x, rooms });
	}
	Ok(homes)
}

/// Current state of every room in a home. Costs one call.
pub async fn states(home: &Home) -> Result<HashMap<i64, RoomState>, ApiError> {
	if home.x {
		let list = request(Method::GET, &format!("{}/homes/{}/rooms", hops_base(), home.id), None).await?;
		Ok(list
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(|r| Some((r.get("id")?.as_i64()?, parse_room_state(r))))
			.collect())
	} else {
		let v = request(Method::GET, &format!("{}/homes/{}/zoneStates", my_base(), home.id), None).await?;
		Ok(v.get("zoneStates")
			.and_then(Value::as_object)
			.into_iter()
			.flatten()
			.filter_map(|(id, z)| Some((id.parse().ok()?, parse_zone_state(z))))
			.collect())
	}
}

/// How long a change made with the dial lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Until {
	NextBlock,
	Manual,
	Timer(u64),
}

impl Until {
	pub fn parse(s: &str) -> Self {
		match s {
			"manual" => Until::Manual,
			"1h" => Until::Timer(3600),
			"3h" => Until::Timer(3 * 3600),
			_ => Until::NextBlock,
		}
	}
}

/// The request body for a temperature change. `target` None turns the heating off.
pub fn overlay_body(x: bool, target: Option<f64>, until: Until) -> Value {
	let kind = match until {
		Until::NextBlock => "NEXT_TIME_BLOCK",
		Until::Manual => "MANUAL",
		Until::Timer(_) => "TIMER",
	};
	let type_key = if x { "type" } else { "typeSkillBasedApp" };
	let mut termination = json!({ type_key: kind });
	if let Until::Timer(secs) = until {
		termination["durationInSeconds"] = json!(secs);
	}
	let setting = match (x, target) {
		(true, Some(t)) => json!({ "power": "ON", "isBoost": false, "temperature": { "value": t } }),
		(true, None) => json!({ "power": "OFF" }),
		(false, Some(t)) => json!({ "type": "HEATING", "power": "ON", "temperature": { "celsius": t } }),
		(false, None) => json!({ "type": "HEATING", "power": "OFF" }),
	};
	json!({ "setting": setting, "termination": termination })
}

pub async fn set_target(home: &Home, room: i64, target: Option<f64>, until: Until) -> Result<(), ApiError> {
	let body = overlay_body(home.x, target, until);
	if home.x {
		let url = format!("{}/homes/{}/rooms/{room}/manualControl", hops_base(), home.id);
		request(Method::POST, &url, Some(&body)).await?;
	} else {
		let url = format!("{}/homes/{}/zones/{room}/overlay", my_base(), home.id);
		request(Method::PUT, &url, Some(&body)).await?;
	}
	Ok(())
}

/// Drops the manual change so the room follows its schedule again.
pub async fn resume_schedule(home: &Home, room: i64) -> Result<(), ApiError> {
	if home.x {
		let url = format!("{}/homes/{}/rooms/{room}/resumeSchedule", hops_base(), home.id);
		request(Method::POST, &url, None).await?;
	} else {
		let url = format!("{}/homes/{}/zones/{room}/overlay", my_base(), home.id);
		request(Method::DELETE, &url, None).await?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ratelimit_header() {
		let q = parse_ratelimit("\"perday\";r=87");
		assert_eq!(q.remaining, Some(87));
		assert!(q.refill.is_none());
		let q = parse_ratelimit("\"perday\";r=0;t=120");
		assert_eq!(q.remaining, Some(0));
		assert!(q.refill.is_some());
	}

	#[test]
	fn classic_state() {
		let v = json!({
			"setting": { "type": "HEATING", "power": "ON", "temperature": { "celsius": 21.5 } },
			"overlayType": "MANUAL", "overlay": { "type": "MANUAL" },
			"activityDataPoints": { "heatingPower": { "percentage": 40.0 } },
			"sensorDataPoints": { "insideTemperature": { "celsius": 19.84 }, "humidity": { "percentage": 51.2 } }
		});
		let s = parse_zone_state(&v);
		assert_eq!(s.current, Some(19.84));
		assert_eq!(s.target, Some(21.5));
		assert_eq!(s.heating, Some(40.0));
		assert!(s.manual);
		let off = json!({ "setting": { "type": "HEATING", "power": "OFF", "temperature": null }, "overlayType": null });
		assert_eq!(parse_zone_state(&off).target, None);
		assert!(!parse_zone_state(&off).manual);
	}

	#[test]
	fn x_state() {
		let v = json!({
			"id": 1, "name": "Living room",
			"sensorDataPoints": { "insideTemperature": { "value": 20.1 }, "humidity": { "percentage": 48 } },
			"setting": { "power": "ON", "temperature": { "value": 19.0 } },
			"heatingPower": { "percentage": 0 },
			"manualControlTermination": null
		});
		let s = parse_room_state(&v);
		assert_eq!(s.current, Some(20.1));
		assert_eq!(s.target, Some(19.0));
		assert!(!s.manual);
	}

	#[test]
	fn bodies() {
		assert_eq!(
			overlay_body(false, Some(21.0), Until::NextBlock),
			json!({ "setting": { "type": "HEATING", "power": "ON", "temperature": { "celsius": 21.0 } },
				"termination": { "typeSkillBasedApp": "NEXT_TIME_BLOCK" } })
		);
		assert_eq!(
			overlay_body(true, Some(19.5), Until::Timer(3600)),
			json!({ "setting": { "power": "ON", "isBoost": false, "temperature": { "value": 19.5 } },
				"termination": { "type": "TIMER", "durationInSeconds": 3600 } })
		);
		assert_eq!(
			overlay_body(true, None, Until::Manual),
			json!({ "setting": { "power": "OFF" }, "termination": { "type": "MANUAL" } })
		);
	}
}
