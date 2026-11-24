//! Everything the plugin knows: homes and rooms, their latest state, which room each device has
//! selected, and changes waiting to be sent. Also the background refresh and the debounced writes.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use openaction::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{self, ApiError, Home, RoomState, Until};
use crate::{auth, files, render};

pub const ROOM_UUID: &str = "io.github.gauravk-in.tado.room";
pub const DIAL_UUID: &str = "io.github.gauravk-in.tado.dial";

/// Wait this long after the last dial tick before sending the new temperature, so one turn is one API call.
const SEND_DELAY: Duration = Duration::from_millis(1500);
/// Never ask tado° for a home's state more often than this, even on request.
const MIN_GAP: Duration = Duration::from_secs(20);
const MIN_C: f64 = 5.0;

pub type RoomKey = (i64, i64);

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct RoomSettings {
	pub home: Option<i64>,
	pub room: Option<i64>,
	/// Shown instead of the room's name when set.
	pub label: String,
}

impl RoomSettings {
	pub fn key(&self) -> Option<RoomKey> {
		Some((self.home?, self.room?))
	}
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct DialSettings {
	/// Degrees per dial tick.
	pub step: f64,
	/// How long a change lasts: "nextBlock", "manual", "1h", "3h".
	pub until: String,
	/// Pressing the dial: "resume" (back to schedule), "off" (heating off / back to schedule), "none".
	pub press: String,
	pub reverse: bool,
}

impl Default for DialSettings {
	fn default() -> Self {
		Self { step: 0.5, until: "nextBlock".into(), press: "resume".into(), reverse: false }
	}
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
	/// Minutes between automatic refreshes; 0 refreshes only on request.
	pub interval_min: u32,
}

impl Default for Prefs {
	fn default() -> Self {
		Self { interval_min: 30 }
	}
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Login {
	user_code: String,
	url: String,
	qr: Option<String>,
}

#[derive(Default)]
struct Store {
	signed_in: bool,
	homes: Vec<Home>,
	states: HashMap<RoomKey, RoomState>,
	/// Target being dialled in but not sent yet, with a counter that tells turns apart.
	pending: HashMap<RoomKey, (Option<f64>, u64)>,
	counter: u64,
	rooms: HashMap<String, RoomSettings>,
	dials: HashMap<String, DialSettings>,
	selected: HashMap<String, RoomKey>,
	polled: HashMap<i64, Instant>,
	forced: HashSet<i64>,
	error: Option<String>,
	login: Option<Login>,
	login_run: u64,
	inspectors: HashSet<String>,
	prefs: Prefs,
	/// Last image and title sent per instance, so unchanged keys are not redrawn.
	drawn: HashMap<String, String>,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| {
	Mutex::new(Store {
		homes: files::load(files::HOMES).unwrap_or_default(),
		prefs: files::load(files::PREFS).unwrap_or_default(),
		..Default::default()
	})
});

fn with<T>(f: impl FnOnce(&mut Store) -> T) -> T {
	f(&mut STORE.lock().unwrap())
}

pub async fn init() {
	let signed_in = auth::signed_in().await;
	with(|s| s.signed_in = signed_in);
	tokio::spawn(refresh_loop());
}

// ---------- instances ----------

pub fn set_room(instance: &Instance, settings: &RoomSettings) {
	with(|s| {
		s.rooms.insert(instance.instance_id.clone(), settings.clone());
		s.drawn.remove(&instance.instance_id);
		if let Some(key) = settings.key() {
			if !s.states.contains_key(&key) {
				s.forced.insert(key.0);
			}
			s.selected.entry(instance.device_id.clone()).or_insert(key);
		}
	});
}

pub fn set_dial(instance: &Instance, settings: &DialSettings) {
	with(|s| {
		s.dials.insert(instance.instance_id.clone(), settings.clone());
		s.drawn.remove(&instance.instance_id);
	});
}

pub fn forget(instance: &Instance) {
	with(|s| {
		s.rooms.remove(&instance.instance_id);
		s.dials.remove(&instance.instance_id);
		s.drawn.remove(&instance.instance_id);
		s.inspectors.remove(&instance.instance_id);
	});
}

fn sorted(mut v: Vec<std::sync::Arc<Instance>>) -> Vec<std::sync::Arc<Instance>> {
	v.sort_by_key(|i| i.coordinates.as_ref().map(|c| (c.row, c.column)).unwrap_or((u8::MAX, u8::MAX)));
	v
}

/// Redraws every visible room key and dial whose picture changed.
pub async fn redraw() {
	let rooms = sorted(visible_instances(ROOM_UUID).await);
	let dials = visible_instances(DIAL_UUID).await;
	let mut jobs = Vec::new();
	with(|s| {
		for i in &rooms {
			let settings = s.rooms.get(&i.instance_id).cloned().unwrap_or_default();
			let img = room_image(s, &i.device_id, &settings);
			let url = render::data_url(&img);
			if s.drawn.get(&i.instance_id) != Some(&url) {
				s.drawn.insert(i.instance_id.clone(), url.clone());
				jobs.push((i.clone(), url, true));
			}
		}
		for i in &dials {
			let title = dial_title(s, &i.device_id);
			if s.drawn.get(&i.instance_id) != Some(&title) {
				s.drawn.insert(i.instance_id.clone(), title.clone());
				jobs.push((i.clone(), title, false));
			}
		}
	});
	for (instance, value, image) in jobs {
		let result =
			if image { instance.set_image(Some(value), None).await } else { instance.set_title(Some(value), None).await };
		if let Err(e) = result {
			log::warn!("Could not update {}: {e}", instance.instance_id);
		}
	}
}

/// Redraw after a short delay: keys that were just placed are not ready for images straight away.
pub fn redraw_later() {
	tokio::spawn(async {
		tokio::time::sleep(Duration::from_millis(300)).await;
		redraw().await;
	});
}

fn room_name(s: &Store, key: RoomKey) -> Option<String> {
	s.homes.iter().find(|h| h.id == key.0)?.rooms.iter().find(|r| r.id == key.1).map(|r| r.name.clone())
}

fn room_image(s: &Store, device: &str, settings: &RoomSettings) -> image::RgbaImage {
	let Some(key) = settings.key() else {
		return render::message("tado°", if s.signed_in { "Choose a room" } else { "Sign in first" });
	};
	let name = if settings.label.trim().is_empty() {
		room_name(s, key).unwrap_or_else(|| "Room".into())
	} else {
		settings.label.trim().to_owned()
	};
	let state = s.states.get(&key);
	let notice = if !s.signed_in {
		Some("Sign in")
	} else if state.is_none() && s.error.is_some() {
		Some("No data")
	} else {
		None
	};
	render::draw(&render::Key {
		name: &name,
		state,
		selected: s.selected.get(device) == Some(&key),
		pending: s.pending.get(&key).map(|p| p.0),
		notice,
	})
}

fn dial_title(s: &Store, device: &str) -> String {
	let Some(key) = s.selected.get(device).copied() else { return "tado°".into() };
	let name = room_name(s, key).unwrap_or_else(|| "Room".into());
	let target = s.pending.get(&key).map(|p| p.0).or_else(|| s.states.get(&key).map(|st| st.target));
	match target {
		Some(Some(t)) => format!("{name}\n{}", render::temperature(t)),
		Some(None) => format!("{name}\nOff"),
		None => name,
	}
}

// ---------- key and dial input ----------

/// Pressing a room key selects it for the dials on the same device; pressing it again refreshes it.
pub async fn press_room(instance: &Instance, settings: &RoomSettings) {
	let Some(key) = settings.key() else { return };
	with(|s| {
		if s.selected.get(&instance.device_id) == Some(&key) {
			s.forced.insert(key.0);
		}
		s.selected.insert(instance.device_id.clone(), key);
	});
	redraw().await;
}

/// The room the dials of a device control: the selected one, else the first room key on the device.
async fn target_room(device: &str) -> Option<RoomKey> {
	if let Some(key) = with(|s| s.selected.get(device).copied()) {
		return Some(key);
	}
	let rooms = sorted(visible_instances(ROOM_UUID).await);
	with(|s| {
		let key = rooms
			.iter()
			.filter(|i| i.device_id == device)
			.find_map(|i| s.rooms.get(&i.instance_id).and_then(RoomSettings::key))?;
		s.selected.insert(device.to_owned(), key);
		Some(key)
	})
}

fn home(s: &Store, id: i64) -> Option<Home> {
	s.homes.iter().find(|h| h.id == id).cloned()
}

fn round(t: f64) -> f64 {
	(t * 10.0).round() / 10.0
}

/// Next target after `ticks` dial ticks. Below the minimum the heating turns off.
pub fn step_target(current: Option<f64>, ticks: i16, step: f64, max: f64) -> Option<f64> {
	let step = if step > 0.0 { step } else { 0.5 };
	match current {
		None if ticks > 0 => Some(round((MIN_C + step * (ticks - 1) as f64).min(max))),
		None => None,
		Some(t) => {
			let next = round(t + step * ticks as f64);
			if next < MIN_C - 1e-6 { None } else { Some(next.min(max)) }
		}
	}
}

pub async fn turn_dial(instance: &Instance, settings: &DialSettings, ticks: i16) {
	if ticks == 0 {
		return;
	}
	let ticks = if settings.reverse { -ticks } else { ticks };
	let Some(key) = target_room(&instance.device_id).await else {
		let _ = instance.show_alert().await;
		return;
	};
	let counter = with(|s| {
		let max = if home(s, key.0).is_some_and(|h| h.x) { 30.0 } else { 25.0 };
		let base = match s.pending.get(&key) {
			Some((t, _)) => *t,
			None => s.states.get(&key).map(|st| st.target).unwrap_or(Some(20.0)),
		};
		s.counter += 1;
		let counter = s.counter;
		s.pending.insert(key, (step_target(base, ticks, settings.step, max), counter));
		counter
	});
	redraw().await;

	let instance_id = instance.instance_id.clone();
	let until = Until::parse(&settings.until);
	tokio::spawn(async move {
		tokio::time::sleep(SEND_DELAY).await;
		let job = with(|s| match s.pending.get(&key) {
			Some((target, c)) if *c == counter => Some((*target, home(s, key.0))),
			_ => None,
		});
		let Some((target, home)) = job else { return };
		let result = match home {
			Some(home) => api::set_target(&home, key.1, target, until).await,
			None => Err(ApiError::Http(404)),
		};
		finish_write(key, counter, &instance_id, result, |st| {
			st.target = target;
			st.manual = true;
		})
		.await;
	});
}

async fn finish_write(
	key: RoomKey,
	counter: u64,
	instance_id: &str,
	result: Result<(), ApiError>,
	apply: impl FnOnce(&mut RoomState),
) {
	let ok = result.is_ok();
	with(|s| {
		if s.pending.get(&key).is_some_and(|(_, c)| *c == counter) {
			s.pending.remove(&key);
		}
		match &result {
			Ok(()) => {
				apply(s.states.entry(key).or_default());
				s.error = None;
			}
			Err(e) => note_error(s, e),
		}
	});
	if let Some(instance) = get_instance(instance_id.to_owned()).await {
		let _ = if ok { instance.show_ok().await } else { instance.show_alert().await };
	}
	redraw().await;
	broadcast().await;
}

pub async fn press_dial(instance: &Instance, settings: &DialSettings) {
	if settings.press == "none" {
		return;
	}
	let Some(key) = target_room(&instance.device_id).await else {
		let _ = instance.show_alert().await;
		return;
	};
	let (home, current) = with(|s| {
		s.pending.remove(&key);
		(home(s, key.0), s.states.get(&key).cloned().unwrap_or_default())
	});
	let Some(home) = home else { return };
	if settings.press == "off" && current.target.is_some() {
		let until = Until::parse(&settings.until);
		let result = api::set_target(&home, key.1, None, until).await;
		finish_write(key, 0, &instance.instance_id, result, |st| {
			st.target = None;
			st.manual = true;
		})
		.await;
		return;
	}
	let result = api::resume_schedule(&home, key.1).await;
	let ok = result.is_ok();
	finish_write(key, 0, &instance.instance_id, result, |st| st.manual = false).await;
	if ok {
		// The schedule's temperature is only known after asking again.
		tokio::spawn(async move {
			tokio::time::sleep(Duration::from_secs(3)).await;
			with(|s| s.forced.insert(key.0));
		});
	}
}

// ---------- refreshing ----------

fn note_error(s: &mut Store, e: &ApiError) {
	log::warn!("{e}");
	if matches!(e, ApiError::SignedOut) {
		s.signed_in = false;
	}
	s.error = Some(match e {
		ApiError::RateLimited(Some(when)) => {
			let mins = when.saturating_duration_since(Instant::now()).as_secs().div_ceil(60);
			format!("{e}. It refills in about {} h {} min.", mins / 60, mins % 60)
		}
		_ => e.to_string(),
	});
}

async fn load_homes() -> Result<(), ApiError> {
	let homes = api::homes().await;
	with(|s| match &homes {
		Ok(homes) => {
			files::save(files::HOMES, homes, false);
			s.homes = homes.clone();
			s.error = None;
		}
		Err(e) => note_error(s, e),
	});
	homes.map(|_| ())
}

async fn refresh_loop() {
	loop {
		refresh_due().await;
		tokio::time::sleep(Duration::from_secs(5)).await;
	}
}

async fn refresh_due() {
	if !with(|s| s.signed_in) {
		return;
	}
	let visible = visible_instances(ROOM_UUID).await;
	let (wanted, need_homes) = with(|s| {
		let wanted: HashSet<i64> = visible
			.iter()
			.filter_map(|i| s.rooms.get(&i.instance_id).and_then(RoomSettings::key))
			.map(|k| k.0)
			.collect();
		let need = s.homes.is_empty() && (!wanted.is_empty() || !s.inspectors.is_empty());
		(wanted, need)
	});
	if need_homes && load_homes().await.is_err() {
		with(|s| s.forced.clear());
		broadcast().await;
		return;
	}
	let due: Vec<Home> = with(|s| {
		let interval = Duration::from_secs(s.prefs.interval_min as u64 * 60);
		let now = Instant::now();
		s.homes
			.iter()
			.filter(|h| wanted.contains(&h.id))
			.filter(|h| match s.polled.get(&h.id) {
				None => true,
				Some(t) => {
					let age = now - *t;
					age >= MIN_GAP && (s.forced.contains(&h.id) || (interval > Duration::ZERO && age >= interval))
				}
			})
			.cloned()
			.collect()
	});
	for home in due {
		let result = api::states(&home).await;
		with(|s| {
			s.polled.insert(home.id, Instant::now());
			s.forced.remove(&home.id);
			match &result {
				Ok(states) => {
					for (room, state) in states {
						s.states.insert((home.id, *room), state.clone());
					}
					s.error = None;
				}
				Err(e) => note_error(s, e),
			}
		});
		redraw().await;
		broadcast().await;
	}
}

// ---------- property inspector ----------

fn status(s: &Store) -> Value {
	let quota = *api::QUOTA.lock().unwrap();
	json!({
		"status": {
			"signedIn": s.signed_in,
			"homes": s.homes,
			"error": s.error,
			"login": s.login,
			"remaining": quota.remaining,
			"intervalMin": s.prefs.interval_min,
		}
	})
}

/// Sends the account status to every open property inspector.
pub async fn broadcast() {
	let (ids, status) = with(|s| (s.inspectors.iter().cloned().collect::<Vec<_>>(), status(s)));
	for id in ids {
		if let Some(instance) = get_instance(id).await {
			let _ = instance.send_to_property_inspector(status.clone()).await;
		}
	}
}

pub async fn inspector_opened(instance: &Instance) {
	with(|s| s.inspectors.insert(instance.instance_id.clone()));
	broadcast().await;
}

pub fn inspector_closed(instance: &Instance) {
	with(|s| s.inspectors.remove(&instance.instance_id));
}

pub async fn from_inspector(instance: &Instance, payload: &Value) {
	match payload.get("cmd").and_then(Value::as_str).unwrap_or_default() {
		"status" => inspector_opened(instance).await,
		"login" => {
			tokio::spawn(sign_in());
		}
		"cancelLogin" => {
			with(|s| {
				s.login = None;
				s.login_run += 1;
			});
			broadcast().await;
		}
		"logout" => {
			auth::sign_out().await;
			with(|s| {
				s.signed_in = false;
				s.states.clear();
				s.polled.clear();
				s.error = None;
			});
			redraw().await;
			broadcast().await;
		}
		"reloadRooms" => {
			let _ = load_homes().await;
			with(|s| s.forced.extend(s.homes.iter().map(|h| h.id)));
			redraw().await;
			broadcast().await;
		}
		"setInterval" => {
			let minutes = payload.get("minutes").and_then(Value::as_u64).unwrap_or(30) as u32;
			with(|s| {
				s.prefs.interval_min = minutes;
				files::save(files::PREFS, &s.prefs, false);
			});
			broadcast().await;
		}
		other => log::warn!("Unknown inspector command {other:?}"),
	}
}

async fn sign_in() {
	let code = match auth::start_sign_in().await {
		Ok(code) => code,
		Err(e) => {
			with(|s| s.error = Some(e));
			broadcast().await;
			return;
		}
	};
	let run = with(|s| {
		s.login_run += 1;
		s.login = Some(Login {
			user_code: code.user_code.clone(),
			url: code.verification_uri_complete.clone(),
			qr: render::qr_svg(&code.verification_uri_complete),
		});
		s.error = None;
		s.login_run
	});
	broadcast().await;
	let _ = open_url(code.verification_uri_complete.clone()).await;

	let deadline = Instant::now() + Duration::from_secs(code.expires_in);
	let mut interval = code.interval.max(1);
	let outcome = loop {
		tokio::time::sleep(Duration::from_secs(interval)).await;
		if with(|s| s.login_run != run) {
			return;
		}
		if Instant::now() > deadline {
			break Err("The code expired. Start the sign-in again.".to_owned());
		}
		match auth::poll_sign_in(&code).await {
			auth::Poll::Done => break Ok(()),
			auth::Poll::Pending => {}
			auth::Poll::SlowDown => interval += 5,
			auth::Poll::Failed(e) => break Err(e),
		}
	};
	with(|s| {
		s.login = None;
		match &outcome {
			Ok(()) => {
				s.signed_in = true;
				s.error = None;
				s.drawn.clear();
			}
			Err(e) => s.error = Some(e.clone()),
		}
	});
	broadcast().await;
	if outcome.is_ok() {
		let _ = load_homes().await;
		broadcast().await;
		redraw().await;
	}
}

#[cfg(test)]
mod tests {
	use super::step_target;

	#[test]
	fn dial_steps() {
		assert_eq!(step_target(Some(20.0), 1, 0.5, 25.0), Some(20.5));
		assert_eq!(step_target(Some(20.0), -3, 0.5, 25.0), Some(18.5));
		assert_eq!(step_target(Some(24.5), 4, 0.5, 25.0), Some(25.0));
		assert_eq!(step_target(Some(5.0), -1, 0.5, 25.0), None);
		assert_eq!(step_target(None, -1, 0.5, 25.0), None);
		assert_eq!(step_target(None, 1, 0.5, 25.0), Some(5.0));
		assert_eq!(step_target(None, 3, 1.0, 25.0), Some(7.0));
		assert_eq!(step_target(Some(20.0), 1, 0.1, 25.0), Some(20.1));
	}
}
