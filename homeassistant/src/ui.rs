//! The deck's screens and what each key and knob does on them. Pure logic: it reads the [`Home`] and returns
//! what to draw and which service calls to make, so it can be tested without a deck or Home Assistant.
//!
//! Screens: Home (all rooms) -> Room (its devices) -> Device (controls for one device). After a period
//! without input the deck goes back to Home.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::ha::ServiceCall;
use crate::layout::{Layout, Nav};
use crate::model::{EntityState, Home, Kind, Room, Status, fmt_num};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
	Home,
	Room(String),
	Entity { room: String, entity: String },
}

/// One thing that can sit on a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
	Status,
	Room(String),
	/// All lights in the room.
	RoomLights(String),
	Entity(String),
	Control(Control),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
	/// The device itself: state and main value. Tapping toggles.
	Main,
	HvacMode(String),
	Preset(String),
	TempDown,
	TempUp,
	Brightness(u8),
	ColourTemp(&'static str, u32),
	CoverOpen,
	CoverStop,
	CoverClose,
	FanSpeed(u8),
	MediaPrevious,
	MediaPlayPause,
	MediaNext,
	MediaMute,
	VolumeDown,
	VolumeUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
	Neutral,
	Room,
	Heat,
	Cool,
	Light,
	Cover,
	Fan,
	Media,
	Switch,
	Scene,
	Warning,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Tile {
	pub icon: String,
	/// Big text in the middle (a temperature, a percentage, ON/OFF).
	pub value: String,
	/// The name along the bottom.
	pub title: String,
	/// Small text under the value.
	pub sub: String,
	pub accent: Option<Accent>,
	/// Lit (on, open, playing, selected mode).
	pub on: bool,
	/// Highlighted by the knob.
	pub focused: bool,
	/// 0..1, drawn as a bar along the bottom.
	pub level: Option<f32>,
	pub dim: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyView {
	Blank,
	Tile(Tile),
	Nav(Nav, bool),
}

/// A value being changed with a knob: shown right away, sent to HA once the knob stops.
#[derive(Clone, Debug)]
struct Pending {
	call: ServiceCall,
	field: &'static str,
	value: f64,
	integer: bool,
	due: Instant,
	sent: Option<Instant>,
}

const DIAL_SETTLE: Duration = Duration::from_millis(350);
const PENDING_HOLD: Duration = Duration::from_secs(4);

pub struct Ui {
	pub layout: Layout,
	pub screen: Screen,
	focus: Option<usize>,
	page: usize,
	last_input: Instant,
	held: HashMap<usize, (Instant, bool)>,
	pending: HashMap<String, Pending>,
	/// The mode a thermostat had before it was switched off, so pressing the knob turns it back on as it was.
	last_mode: HashMap<String, String>,
}

impl Ui {
	pub fn new(layout: Layout) -> Self {
		Self {
			layout,
			screen: Screen::Home,
			focus: None,
			page: 0,
			last_input: Instant::now(),
			held: HashMap::new(),
			pending: HashMap::new(),
			last_mode: HashMap::new(),
		}
	}

	// ----- Items and paging -----

	pub fn items(&self, home: &Home) -> Vec<Item> {
		if home.status != Status::Ready {
			return vec![Item::Status];
		}
		match &self.screen {
			Screen::Home => home.rooms.iter().map(|r| Item::Room(r.id.clone())).collect(),
			Screen::Room(id) => {
				let Some(room) = home.room(id) else { return vec![] };
				let mut items = vec![];
				if room.lights().count() >= 2 {
					items.push(Item::RoomLights(room.id.clone()));
				}
				items.extend(room.entities.iter().map(|e| Item::Entity(e.clone())));
				items
			}
			Screen::Entity { entity, .. } => controls(home, entity, self.layout.encoders > 0).into_iter().map(Item::Control).collect(),
		}
	}

	fn at_home(&self) -> bool {
		self.screen == Screen::Home
	}

	fn slots(&self, count: usize) -> crate::layout::Slots {
		self.layout.slots(count, !self.at_home())
	}

	fn pages(&self, count: usize) -> usize {
		let cap = self.slots(count).content.len().max(1);
		count.div_ceil(cap).max(1)
	}

	fn go(&mut self, screen: Screen, focus: Option<usize>, home: &Home) {
		self.screen = screen;
		self.focus = focus;
		let cap = self.slots(self.items(home).len()).content.len().max(1);
		self.page = focus.map(|f| f / cap).unwrap_or(0);
	}

	fn back(&mut self, home: &Home) {
		match self.screen.clone() {
			Screen::Home => {
				self.focus = None;
				self.page = 0;
			}
			Screen::Room(id) => {
				let idx = home.rooms.iter().position(|r| r.id == id);
				self.go(Screen::Home, idx, home);
			}
			Screen::Entity { room, entity } => {
				self.go(Screen::Room(room.clone()), None, home);
				let idx = self.items(home).iter().position(|i| *i == Item::Entity(entity.clone()));
				self.go(Screen::Room(room), idx, home);
			}
		}
	}

	pub fn go_home(&mut self) {
		self.screen = Screen::Home;
		self.focus = None;
		self.page = 0;
	}

	/// Moves to a valid screen when what it shows has disappeared from HA (a room deleted, say).
	pub fn validate(&mut self, home: &Home) {
		let valid = match &self.screen {
			Screen::Home => true,
			Screen::Room(r) => home.room(r).is_some(),
			Screen::Entity { room, entity } => home.room(room).is_some_and(|r| r.entities.contains(entity)),
		};
		if !valid {
			self.go_home();
		}
		let count = self.items(home).len();
		self.page = self.page.min(self.pages(count) - 1);
		if let Some(f) = self.focus
			&& f >= count
		{
			self.focus = None;
		}
	}

	// ----- Drawing -----

	pub fn view(&self, home: &Home) -> Vec<KeyView> {
		let items = self.items(home);
		let slots = self.slots(items.len());
		let pages = self.pages(items.len());
		let cap = slots.content.len().max(1);
		let mut keys = vec![KeyView::Blank; self.layout.keys];
		for (i, pos) in slots.content.iter().enumerate() {
			let idx = self.page * cap + i;
			if let Some(item) = items.get(idx) {
				let mut tile = self.tile(home, item);
				tile.focused = self.focus == Some(idx);
				keys[*pos] = KeyView::Tile(tile);
			}
		}
		for (pos, nav) in slots.nav {
			let enabled = match nav {
				Nav::Back | Nav::Home => !self.at_home(),
				Nav::Next => pages > 1,
			};
			keys[pos] = if self.layout.display[pos] && !enabled && nav != Nav::Next {
				KeyView::Blank
			} else {
				KeyView::Nav(nav, enabled)
			};
			if let (Nav::Next, true) = (nav, enabled)
				&& self.layout.display[pos]
			{
				keys[pos] = KeyView::Tile(Tile {
					icon: "→".into(),
					value: format!("{}/{}", self.page + 1, pages),
					title: "More".into(),
					accent: Some(Accent::Neutral),
					..Default::default()
				});
			}
		}
		keys
	}

	fn pending_value(&self, entity: &str, field: &str) -> Option<f64> {
		self.pending.get(&pending_key(entity, field)).map(|p| p.value)
	}

	fn tile(&self, home: &Home, item: &Item) -> Tile {
		match item {
			Item::Status => {
				let (value, sub) = match home.status {
					Status::NotConfigured => ("Setup", "Add HA URL + token"),
					Status::Connecting => ("…", "Connecting"),
					Status::AuthFailed => ("Token?", "HA refused token"),
					Status::Unreachable => ("Offline", "Retrying"),
					Status::Ready => ("", ""),
				};
				Tile {
					icon: "⌂".into(),
					value: value.into(),
					title: "Home Assistant".into(),
					sub: sub.into(),
					accent: Some(Accent::Warning),
					..Default::default()
				}
			}
			Item::Room(id) => {
				let Some(room) = home.room(id) else { return Tile::default() };
				let lights_on = room.lights().any(|l| home.state(l).is_some_and(EntityState::is_on));
				Tile {
					icon: room_icon(&room.name).into(),
					value: String::new(),
					title: room.name.clone(),
					sub: home.room_summary(room),
					accent: Some(Accent::Room),
					on: lights_on,
					..Default::default()
				}
			}
			Item::RoomLights(id) => {
				let Some(room) = home.room(id) else { return Tile::default() };
				let lights: Vec<&EntityState> = room.lights().filter_map(|l| home.state(l)).collect();
				let on = lights.iter().filter(|l| l.is_on()).count();
				Tile {
					icon: "☼".into(),
					value: if on == 0 { "Off".into() } else { format!("{on}/{}", lights.len()) },
					title: "All lights".into(),
					accent: Some(Accent::Light),
					on: on > 0,
					..Default::default()
				}
			}
			Item::Entity(id) => self.entity_tile(home, id, home.room_of(id)),
			Item::Control(c) => {
				let Screen::Entity { entity, .. } = &self.screen else { return Tile::default() };
				self.control_tile(home, entity, c)
			}
		}
	}

	fn entity_tile(&self, home: &Home, id: &str, room: Option<&Room>) -> Tile {
		let name = home.display_name(id, room);
		let Some(s) = home.state(id) else {
			return Tile { title: name, value: "?".into(), dim: true, ..Default::default() };
		};
		let kind = Kind::of(id).unwrap_or(Kind::Switch);
		let mut t = Tile { title: name, on: s.is_on(), dim: !s.available(), ..Default::default() };
		match kind {
			Kind::Climate => {
				let target = self.pending_value(id, "temperature").or_else(|| s.attr_f64("temperature"));
				t.icon = if s.state == "cool" { "❄" } else { "♨" }.into();
				t.accent = Some(if s.state == "cool" { Accent::Cool } else { Accent::Heat });
				t.value = match (s.state.as_str(), target) {
					("off", _) => "Off".into(),
					(_, Some(v)) => format!("{}°", fmt_num(v)),
					(m, None) => title_case(m),
				};
				t.sub = s.attr_f64("current_temperature").map(|c| format!("now {}°", fmt_num(c))).unwrap_or_default();
				if let (Some(v), Some(lo), Some(hi)) = (target, s.attr_f64("min_temp"), s.attr_f64("max_temp"))
					&& s.state != "off"
					&& hi > lo
				{
					t.level = Some(((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32);
				}
			}
			Kind::Light => {
				t.icon = "☼".into();
				t.accent = Some(Accent::Light);
				let pct = self.pending_value(id, "brightness_pct").or_else(|| s.attr_f64("brightness").map(|b| (b / 255.0 * 100.0).round()));
				t.value = if s.state == "on" { pct.map(|p| format!("{}%", p as i64)).unwrap_or_else(|| "On".into()) } else { "Off".into() };
				if s.state == "on" {
					t.level = pct.map(|p| (p / 100.0) as f32);
				}
			}
			Kind::Cover => {
				t.icon = "☰".into();
				t.accent = Some(Accent::Cover);
				let pos = self.pending_value(id, "position").or_else(|| s.attr_f64("current_position"));
				t.value = match pos {
					Some(p) if s.state != "opening" && s.state != "closing" => format!("{}%", p as i64),
					_ => title_case(&s.state),
				};
				t.level = pos.map(|p| (p / 100.0) as f32);
			}
			Kind::Fan => {
				t.icon = "✻".into();
				t.accent = Some(Accent::Fan);
				let pct = self.pending_value(id, "percentage").or_else(|| s.attr_f64("percentage"));
				t.value = if s.state == "on" { pct.map(|p| format!("{}%", p as i64)).unwrap_or_else(|| "On".into()) } else { "Off".into() };
			}
			Kind::MediaPlayer => {
				t.icon = "♫".into();
				t.accent = Some(Accent::Media);
				t.value = title_case(&s.state);
				t.sub = s.attr_str("media_title").unwrap_or("").to_owned();
				let vol = self.pending_value(id, "volume_level").or_else(|| s.attr_f64("volume_level"));
				t.level = vol.map(|v| v as f32);
			}
			Kind::Switch => {
				t.icon = if s.is_on() { "●" } else { "○" }.into();
				t.accent = Some(Accent::Switch);
				t.value = title_case(&s.state);
			}
			Kind::Lock => {
				t.icon = "■".into();
				t.accent = Some(Accent::Switch);
				t.value = title_case(&s.state);
			}
			Kind::Scene | Kind::Script => {
				t.icon = "▶".into();
				t.accent = Some(Accent::Scene);
				t.on = s.state == "on";
				t.value = String::new();
			}
		}
		t
	}

	fn control_tile(&self, home: &Home, id: &str, c: &Control) -> Tile {
		let Some(s) = home.state(id) else { return Tile::default() };
		let accent = match Kind::of(id) {
			Some(Kind::Climate) => Accent::Heat,
			Some(Kind::Light) => Accent::Light,
			Some(Kind::Cover) => Accent::Cover,
			Some(Kind::Fan) => Accent::Fan,
			Some(Kind::MediaPlayer) => Accent::Media,
			_ => Accent::Switch,
		};
		let simple = |icon: &str, title: &str, on: bool| Tile { icon: icon.into(), title: title.into(), accent: Some(accent), on, ..Default::default() };
		match c {
			Control::Main => self.entity_tile(home, id, home.room_of(id)),
			Control::HvacMode(m) => {
				let mut t = simple(mode_icon(m), &title_case(&m.replace('_', " ")), s.state == *m);
				t.accent = Some(match m.as_str() {
					"cool" => Accent::Cool,
					"off" => Accent::Neutral,
					_ => Accent::Heat,
				});
				t
			}
			Control::Preset(p) => simple("✓", &title_case(&p.replace('_', " ")), s.attr_str("preset_mode") == Some(p)),
			Control::TempDown => simple("▼", "Cooler", false),
			Control::TempUp => simple("▲", "Warmer", false),
			Control::Brightness(p) => {
				let cur = s.attr_f64("brightness").map(|b| (b / 255.0 * 100.0).round() as u8);
				let mut t = simple("☼", &format!("{p}%"), s.state == "on" && cur.is_some_and(|c| c.abs_diff(*p) <= 3));
				t.level = Some(*p as f32 / 100.0);
				t
			}
			Control::ColourTemp(name, k) => {
				let cur = s.attr_f64("color_temp_kelvin");
				simple("◐", name, s.state == "on" && cur.is_some_and(|c| (c - *k as f64).abs() < 250.0))
			}
			Control::CoverOpen => simple("▲", "Open", s.state == "open"),
			Control::CoverStop => simple("■", "Stop", false),
			Control::CoverClose => simple("▼", "Close", s.state == "closed"),
			Control::FanSpeed(p) => {
				let cur = s.attr_f64("percentage");
				simple("✻", &format!("{p}%"), s.state == "on" && cur.is_some_and(|c| (c - *p as f64).abs() < 5.0))
			}
			Control::MediaPrevious => simple("◀", "Previous", false),
			Control::MediaPlayPause => simple(if s.state == "playing" { "❚❚" } else { "▶" }, if s.state == "playing" { "Pause" } else { "Play" }, s.state == "playing"),
			Control::MediaNext => simple("▶", "Next", false),
			Control::MediaMute => simple("✕", "Mute", s.attributes.get("is_volume_muted") == Some(&json!(true))),
			Control::VolumeDown => simple("▼", "Vol −", false),
			Control::VolumeUp => simple("▲", "Vol +", false),
		}
	}

	// ----- Input -----

	fn touch(&mut self, now: Instant) {
		self.last_input = now;
	}

	pub fn key_down(&mut self, pos: usize, now: Instant) {
		self.touch(now);
		self.held.insert(pos, (now, false));
	}

	/// Returns the service calls to make. `hold` is how long a press must last to count as a hold.
	pub fn key_up(&mut self, pos: usize, now: Instant, hold: Duration, home: &Home) -> Vec<ServiceCall> {
		self.touch(now);
		let Some((down, fired)) = self.held.remove(&pos) else { return vec![] };
		if fired {
			return vec![];
		}
		self.press(pos, now.duration_since(down) >= hold, home)
	}

	fn press(&mut self, pos: usize, hold: bool, home: &Home) -> Vec<ServiceCall> {
		let items = self.items(home);
		let slots = self.slots(items.len());
		if let Some((_, nav)) = slots.nav.iter().find(|(p, _)| *p == pos) {
			match (nav, hold) {
				(Nav::Back, true) | (Nav::Home, _) => self.go_home(),
				(Nav::Back, false) => self.back(home),
				(Nav::Next, _) => {
					let pages = self.pages(items.len());
					self.page = (self.page + 1) % pages;
					self.focus = None;
				}
			}
			return vec![];
		}
		let Some(i) = slots.content.iter().position(|p| *p == pos) else { return vec![] };
		let idx = self.page * slots.content.len().max(1) + i;
		let Some(item) = items.get(idx).cloned() else { return vec![] };
		self.focus = None;
		self.activate(item, hold, home)
	}

	fn activate(&mut self, item: Item, hold: bool, home: &Home) -> Vec<ServiceCall> {
		match item {
			Item::Status => vec![],
			Item::Room(id) => {
				self.go(Screen::Room(id), None, home);
				vec![]
			}
			Item::RoomLights(id) => {
				let any_on = home.room(&id).is_some_and(|r| r.lights().any(|l| home.state(l).is_some_and(EntityState::is_on)));
				vec![ServiceCall::area("light", &id, if any_on { "turn_off" } else { "turn_on" }, json!({}))]
			}
			Item::Entity(id) => {
				let kind = Kind::of(&id).unwrap_or(Kind::Switch);
				if (hold || !kind.tap_acts()) && kind.has_detail() {
					let room = self.current_room().unwrap_or_default();
					self.go(Screen::Entity { room, entity: id }, None, home);
					vec![]
				} else {
					self.toggle(&id, home)
				}
			}
			Item::Control(c) => {
				let Screen::Entity { entity, .. } = self.screen.clone() else { return vec![] };
				self.control(&entity, &c, home)
			}
		}
	}

	fn current_room(&self) -> Option<String> {
		match &self.screen {
			Screen::Home => None,
			Screen::Room(r) => Some(r.clone()),
			Screen::Entity { room, .. } => Some(room.clone()),
		}
	}

	fn toggle(&mut self, id: &str, home: &Home) -> Vec<ServiceCall> {
		let state = home.state(id);
		let call = match Kind::of(id) {
			Some(Kind::Scene) | Some(Kind::Script) => ServiceCall::entity(id, "turn_on", json!({})),
			Some(Kind::Lock) => ServiceCall::entity(id, if state.is_some_and(|s| s.state == "locked") { "unlock" } else { "lock" }, json!({})),
			Some(Kind::Cover) => ServiceCall::entity(id, "toggle", json!({})),
			Some(Kind::MediaPlayer) => ServiceCall::entity(id, "media_play_pause", json!({})),
			Some(Kind::Climate) => {
				let Some(s) = state else { return vec![] };
				if s.state == "off" {
					let modes = s.attr_list("hvac_modes");
					let mode = self
						.last_mode
						.get(id)
						.cloned()
						.filter(|m| modes.contains(m))
						.or_else(|| ["heat", "heat_cool", "auto", "cool"].iter().map(|m| m.to_string()).find(|m| modes.contains(m)))
						.unwrap_or_else(|| "heat".into());
					ServiceCall::entity(id, "set_hvac_mode", json!({ "hvac_mode": mode }))
				} else {
					self.last_mode.insert(id.to_owned(), s.state.clone());
					ServiceCall::entity(id, "set_hvac_mode", json!({ "hvac_mode": "off" }))
				}
			}
			_ => ServiceCall::entity(id, "toggle", json!({})),
		};
		vec![call]
	}

	fn control(&mut self, id: &str, c: &Control, home: &Home) -> Vec<ServiceCall> {
		let e = |service: &str, data: Value| vec![ServiceCall::entity(id, service, data)];
		match c {
			Control::Main => self.toggle(id, home),
			Control::HvacMode(m) => e("set_hvac_mode", json!({ "hvac_mode": m })),
			Control::Preset(p) => e("set_preset_mode", json!({ "preset_mode": p })),
			Control::TempDown => {
				self.nudge_temperature(id, -1, home);
				vec![]
			}
			Control::TempUp => {
				self.nudge_temperature(id, 1, home);
				vec![]
			}
			Control::Brightness(p) => e("turn_on", json!({ "brightness_pct": p })),
			Control::ColourTemp(_, k) => e("turn_on", json!({ "color_temp_kelvin": k })),
			Control::CoverOpen => e("open_cover", json!({})),
			Control::CoverStop => e("stop_cover", json!({})),
			Control::CoverClose => e("close_cover", json!({})),
			Control::FanSpeed(p) => e("set_percentage", json!({ "percentage": p })),
			Control::MediaPrevious => e("media_previous_track", json!({})),
			Control::MediaPlayPause => e("media_play_pause", json!({})),
			Control::MediaNext => e("media_next_track", json!({})),
			Control::MediaMute => {
				let muted = home.state(id).is_some_and(|s| s.attributes.get("is_volume_muted") == Some(&json!(true)));
				e("volume_mute", json!({ "is_volume_muted": !muted }))
			}
			Control::VolumeDown => e("volume_down", json!({})),
			Control::VolumeUp => e("volume_up", json!({})),
		}
	}

	/// Knob turned. Knob 0 is the main one (the S6's big knob), 1 and 2 the small ones.
	pub fn dial_rotate(&mut self, dial: usize, ticks: i16, now: Instant, home: &Home) -> Vec<ServiceCall> {
		self.touch(now);
		if ticks == 0 || home.status != Status::Ready {
			return vec![];
		}
		let t = ticks as i64;
		match (self.screen.clone(), dial) {
			(Screen::Home, 0) | (Screen::Room(_), 0) => self.move_focus(t, home),
			(Screen::Home, _) => self.turn_page(t, home),
			(Screen::Room(room), 1) => self.adjust_room_lights(&room, t, now),
			(Screen::Room(room), 2) => {
				if let Some(th) = home.room(&room).and_then(Room::thermostat).cloned() {
					self.adjust(&th, 0, t, now, home);
				} else {
					self.turn_page(t, home);
				}
			}
			(Screen::Room(_), _) => self.turn_page(t, home),
			(Screen::Entity { entity, .. }, d) => self.adjust(&entity, d, t, now, home),
		}
		vec![]
	}

	pub fn dial_press(&mut self, dial: usize, now: Instant, home: &Home) -> Vec<ServiceCall> {
		self.touch(now);
		if home.status != Status::Ready {
			return vec![];
		}
		match (self.screen.clone(), dial) {
			(Screen::Home, 0) | (Screen::Room(_), 0) => {
				let items = self.items(home);
				match self.focus.and_then(|f| items.get(f).cloned()) {
					Some(item) => {
						self.focus = None;
						self.activate(item, false, home)
					}
					None => vec![],
				}
			}
			(Screen::Entity { entity, .. }, 0) => self.toggle(&entity, home),
			(Screen::Room(room), 1) => self.activate(Item::RoomLights(room), false, home),
			(Screen::Room(room), 2) => {
				if let Some(th) = home.room(&room).and_then(Room::thermostat).cloned() {
					self.go(Screen::Entity { room, entity: th }, None, home);
				}
				vec![]
			}
			(Screen::Home, _) => vec![],
			(_, 1) => {
				self.back(home);
				vec![]
			}
			_ => {
				self.go_home();
				vec![]
			}
		}
	}

	fn move_focus(&mut self, ticks: i64, home: &Home) {
		let count = self.items(home).len() as i64;
		if count == 0 {
			return;
		}
		let next = match self.focus {
			None if ticks > 0 => {
				let cap = self.slots(count as usize).content.len().max(1) as i64;
				(self.page as i64 * cap + ticks - 1).min(count - 1)
			}
			None => {
				let cap = self.slots(count as usize).content.len().max(1) as i64;
				((self.page as i64 + 1) * cap + ticks).clamp(0, count - 1)
			}
			Some(f) => (f as i64 + ticks).rem_euclid(count),
		};
		self.focus = Some(next as usize);
		let cap = self.slots(count as usize).content.len().max(1);
		self.page = next as usize / cap;
	}

	fn turn_page(&mut self, ticks: i64, home: &Home) {
		let pages = self.pages(self.items(home).len()) as i64;
		self.page = (self.page as i64 + ticks.signum()).rem_euclid(pages) as usize;
		self.focus = None;
	}

	fn set_pending(&mut self, entity: &str, field: &'static str, value: f64, integer: bool, call: ServiceCall, now: Instant) {
		self.pending.insert(pending_key(entity, field), Pending { call, field, value, integer, due: now + DIAL_SETTLE, sent: None });
	}

	fn adjust_room_lights(&mut self, room: &str, ticks: i64, now: Instant) {
		// Relative change for every light in the room: accumulate steps until the knob stops.
		let key = format!("area:{room}");
		let prev = self.pending.get(&pending_key(&key, "brightness_step_pct")).filter(|p| p.sent.is_none()).map(|p| p.value).unwrap_or(0.0);
		let call = ServiceCall::area("light", room, "turn_on", json!({}));
		self.set_pending(&key, "brightness_step_pct", (prev + 10.0 * ticks as f64).clamp(-100.0, 100.0), true, call, now);
	}

	fn nudge_temperature(&mut self, id: &str, dir: i64, home: &Home) {
		self.adjust(id, 0, dir, Instant::now(), home);
	}

	/// Knob `dial` on a device's screen: 0 = main value, 1 and 2 = secondary values.
	fn adjust(&mut self, id: &str, dial: usize, ticks: i64, now: Instant, home: &Home) {
		let Some(s) = home.state(id).cloned() else { return };
		let t = ticks as f64;
		match (Kind::of(id), dial) {
			(Some(Kind::Climate), 0) => {
				let Some(cur) = self.pending_value(id, "temperature").or_else(|| s.attr_f64("temperature")) else { return };
				let step = s.attr_f64("target_temp_step").unwrap_or(if s.attr_str("temperature_unit") == Some("°F") { 1.0 } else { 0.5 });
				let lo = s.attr_f64("min_temp").unwrap_or(5.0);
				let hi = s.attr_f64("max_temp").unwrap_or(35.0);
				let v = ((cur + step * t) / step).round() * step;
				self.set_pending(id, "temperature", v.clamp(lo, hi), false, ServiceCall::entity(id, "set_temperature", json!({})), now);
			}
			(Some(Kind::Climate), 1) => {
				if let Some(call) = cycle(&s, "hvac_modes", &s.state, ticks, "set_hvac_mode", "hvac_mode") {
					self.queue(id, call, now);
				}
			}
			(Some(Kind::Climate), _) => {
				let (list, attr, service, field) = if s.attributes.get("preset_modes").is_some() {
					("preset_modes", "preset_mode", "set_preset_mode", "preset_mode")
				} else {
					("fan_modes", "fan_mode", "set_fan_mode", "fan_mode")
				};
				if let Some(call) = cycle(&s, list, s.attr_str(attr).unwrap_or(""), ticks, service, field) {
					self.queue(id, call, now);
				}
			}
			(Some(Kind::Light), 0) => {
				let cur = self
					.pending_value(id, "brightness_pct")
					.or_else(|| if s.state == "on" { s.attr_f64("brightness").map(|b| (b / 255.0 * 100.0).round()) } else { Some(0.0) })
					.unwrap_or(0.0);
				let v = (cur + 5.0 * t).clamp(0.0, 100.0);
				self.set_pending(id, "brightness_pct", v, true, ServiceCall::entity(id, "turn_on", json!({})), now);
			}
			(Some(Kind::Light), 1) => {
				let (lo, hi) = (s.attr_f64("min_color_temp_kelvin").unwrap_or(2000.0), s.attr_f64("max_color_temp_kelvin").unwrap_or(6500.0));
				let Some(cur) = self.pending_value(id, "color_temp_kelvin").or_else(|| s.attr_f64("color_temp_kelvin")).or(Some((lo + hi) / 2.0)) else { return };
				let v = (cur + 100.0 * t).clamp(lo, hi);
				self.set_pending(id, "color_temp_kelvin", v, true, ServiceCall::entity(id, "turn_on", json!({})), now);
			}
			(Some(Kind::Light), _) => {
				let hue = s.attributes.get("hs_color").and_then(|v| v.get(0)).and_then(Value::as_f64).unwrap_or(0.0);
				let cur = self.pending_value(id, "hue").unwrap_or(hue);
				let v = (cur + 10.0 * t).rem_euclid(360.0);
				// Sent as hs_color [hue, 100]; see `flush`.
				self.set_pending(id, "hue", v, false, ServiceCall::entity(id, "turn_on", json!({})), now);
			}
			(Some(Kind::Cover), _) => {
				let Some(cur) = self.pending_value(id, "position").or_else(|| s.attr_f64("current_position")) else { return };
				let v = (cur + 5.0 * t).clamp(0.0, 100.0);
				self.set_pending(id, "position", v, true, ServiceCall::entity(id, "set_cover_position", json!({})), now);
			}
			(Some(Kind::Fan), _) => {
				let step = s.attr_f64("percentage_step").unwrap_or(10.0).max(1.0);
				let cur = self.pending_value(id, "percentage").or_else(|| s.attr_f64("percentage")).unwrap_or(0.0);
				let v = (((cur + step * t) / step).round() * step).clamp(0.0, 100.0);
				self.set_pending(id, "percentage", v, true, ServiceCall::entity(id, "set_percentage", json!({})), now);
			}
			(Some(Kind::MediaPlayer), 0) => {
				let cur = self.pending_value(id, "volume_level").or_else(|| s.attr_f64("volume_level")).unwrap_or(0.0);
				let v = (cur + 0.02 * t).clamp(0.0, 1.0);
				self.set_pending(id, "volume_level", (v * 100.0).round() / 100.0, false, ServiceCall::entity(id, "volume_set", json!({})), now);
			}
			(Some(Kind::MediaPlayer), 1) => {
				if let Some(call) = cycle(&s, "source_list", s.attr_str("source").unwrap_or(""), ticks, "select_source", "source") {
					self.queue(id, call, now);
				}
			}
			(Some(Kind::MediaPlayer), _) => {
				let service = if ticks > 0 { "media_next_track" } else { "media_previous_track" };
				self.queue(id, ServiceCall::entity(id, service, json!({})), now);
			}
			_ => {}
		}
	}

	/// A one-off call sent once the knob settles (so quick turns through a list only send the last choice).
	fn queue(&mut self, id: &str, call: ServiceCall, now: Instant) {
		self.pending.insert(pending_key(id, "call"), Pending { call, field: "", value: 0.0, integer: false, due: now + DIAL_SETTLE, sent: None });
	}

	/// Called a few times a second: sends settled knob values, fires holds, and handles the idle timeout.
	/// Returns whether to redraw and the calls to make.
	pub fn tick(&mut self, now: Instant, hold: Duration, timeout: Option<Duration>, home: &Home) -> (bool, Vec<ServiceCall>) {
		let mut redraw = false;
		let mut calls = vec![];

		let mut done = vec![];
		for (key, p) in self.pending.iter_mut() {
			match p.sent {
				None if now >= p.due => {
					let mut call = p.call.clone();
					if !p.field.is_empty() {
						let value = if p.integer { json!(p.value.round() as i64) } else { json!(p.value) };
						if p.field == "hue" {
							call.data["hs_color"] = json!([p.value.round(), 100]);
						} else {
							call.data[p.field] = value;
						}
					}
					calls.push(call);
					p.sent = Some(now);
					if p.field.is_empty() || p.field == "brightness_step_pct" {
						done.push(key.clone());
					}
				}
				Some(sent) if now.duration_since(sent) >= PENDING_HOLD => {
					done.push(key.clone());
					redraw = true;
				}
				_ => {}
			}
		}
		for k in done {
			self.pending.remove(&k);
		}

		let fire: Vec<usize> = self.held.iter().filter(|(_, (down, fired))| !fired && now.duration_since(*down) >= hold).map(|(p, _)| *p).collect();
		for pos in fire {
			if let Some(h) = self.held.get_mut(&pos) {
				h.1 = true;
			}
			calls.extend(self.press(pos, true, home));
			redraw = true;
		}

		if let Some(t) = timeout
			&& (self.screen != Screen::Home || self.focus.is_some() || self.page != 0)
			&& self.held.is_empty()
			&& now.duration_since(self.last_input) >= t
		{
			self.go_home();
			redraw = true;
		}
		(redraw, calls)
	}

	/// Drops a knob value once HA reports it, so the key shows the real state again.
	pub fn state_changed(&mut self, entity: &str, home: &Home) {
		let Some(s) = home.state(entity) else { return };
		self.pending.retain(|_, p| {
			if p.sent.is_none() || p.call.target.get("entity_id").and_then(Value::as_str) != Some(entity) {
				return true;
			}
			let actual = match p.field {
				"temperature" => s.attr_f64("temperature"),
				"brightness_pct" => s.attr_f64("brightness").map(|b| (b / 255.0 * 100.0).round()).or(Some(0.0)),
				"position" => s.attr_f64("current_position"),
				"percentage" => s.attr_f64("percentage"),
				"volume_level" => s.attr_f64("volume_level"),
				"color_temp_kelvin" => s.attr_f64("color_temp_kelvin"),
				_ => None,
			};
			!actual.is_some_and(|a| (a - p.value).abs() < 1.5)
		});
	}
}

fn pending_key(entity: &str, field: &str) -> String {
	format!("{entity}|{field}")
}

fn cycle(s: &EntityState, list: &str, current: &str, ticks: i64, service: &str, field: &str) -> Option<ServiceCall> {
	let options = s.attr_list(list);
	if options.is_empty() {
		return None;
	}
	let idx = options.iter().position(|o| o == current).map(|i| i as i64).unwrap_or(-1);
	let next = (idx + ticks.signum()).rem_euclid(options.len() as i64) as usize;
	Some(ServiceCall::entity(&s.entity_id, service, json!({ field: options[next] })))
}

/// The keys on a device's screen. With knobs, the main value is set by turning; without, it gets −/+ keys.
pub fn controls(home: &Home, id: &str, has_knobs: bool) -> Vec<Control> {
	let Some(s) = home.state(id) else { return vec![Control::Main] };
	let mut c = vec![Control::Main];
	match Kind::of(id) {
		Some(Kind::Climate) => {
			if !has_knobs {
				c.extend([Control::TempDown, Control::TempUp]);
			}
			c.extend(s.attr_list("hvac_modes").into_iter().map(Control::HvacMode));
			c.extend(s.attr_list("preset_modes").into_iter().filter(|p| p != "none").map(Control::Preset));
		}
		Some(Kind::Light) => {
			let modes = s.attr_list("supported_color_modes");
			if modes.iter().any(|m| m != "onoff") {
				c.extend([25, 50, 75, 100].map(Control::Brightness));
			}
			if modes.iter().any(|m| matches!(m.as_str(), "color_temp" | "rgbww" | "rgbw" | "rgb" | "hs" | "xy")) && s.attributes.get("min_color_temp_kelvin").is_some() {
				c.extend([Control::ColourTemp("Warm", 2700), Control::ColourTemp("Neutral", 4000), Control::ColourTemp("Cool", 6000)]);
			}
		}
		Some(Kind::Cover) => c.extend([Control::CoverOpen, Control::CoverStop, Control::CoverClose]),
		Some(Kind::Fan) => c.extend([25, 50, 75, 100].map(Control::FanSpeed)),
		Some(Kind::MediaPlayer) => {
			c.extend([Control::MediaPrevious, Control::MediaPlayPause, Control::MediaNext, Control::MediaMute]);
			if !has_knobs {
				c.extend([Control::VolumeDown, Control::VolumeUp]);
			}
		}
		_ => {}
	}
	c
}

fn mode_icon(mode: &str) -> &'static str {
	match mode {
		"heat" => "♨",
		"cool" => "❄",
		"off" => "○",
		"dry" => "∿",
		"fan_only" => "✻",
		_ => "⇄",
	}
}

fn room_icon(name: &str) -> &'static str {
	let n = name.to_lowercase();
	if n.contains("kitchen") {
		"♨"
	} else if n.contains("bed") {
		"☾"
	} else if n.contains("garden") || n.contains("outdoor") || n.contains("patio") || n.contains("terrace") {
		"☀"
	} else if n.contains("office") || n.contains("study") {
		"✎"
	} else {
		"⌂"
	}
}

fn title_case(s: &str) -> String {
	let mut c = s.chars();
	match c.next() {
		Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
		None => String::new(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::layout::Layout;
	use crate::model::tests::sample_home;

	const HOLD: Duration = Duration::from_millis(500);

	fn s6() -> Ui {
		Ui::new(Layout::streonor_s6())
	}

	fn tap(ui: &mut Ui, pos: usize, home: &Home) -> Vec<ServiceCall> {
		let now = Instant::now();
		ui.key_down(pos, now);
		ui.key_up(pos, now + Duration::from_millis(50), HOLD, home)
	}

	fn title(v: &KeyView) -> &str {
		match v {
			KeyView::Tile(t) => &t.title,
			_ => "",
		}
	}

	#[test]
	fn home_shows_rooms_and_nav_keys_on_plain_buttons() {
		let home = sample_home();
		let ui = s6();
		let v = ui.view(&home);
		assert_eq!(title(&v[0]), "Living Room");
		assert_eq!(title(&v[1]), "Bedroom");
		assert_eq!(v[2], KeyView::Blank);
		assert_eq!(v[6], KeyView::Nav(Nav::Back, false));
		assert_eq!(v[7], KeyView::Nav(Nav::Home, false));
		assert_eq!(v[8], KeyView::Nav(Nav::Next, false));
	}

	#[test]
	fn pick_room_then_thermostat_then_set_temperature_then_time_out() {
		let home = sample_home();
		let mut ui = s6();
		let t0 = Instant::now();

		// Big knob: focus the first room, press to open.
		ui.dial_rotate(0, 1, t0, &home);
		assert!(matches!(&ui.view(&home)[0], KeyView::Tile(t) if t.focused));
		ui.dial_press(0, t0, &home);
		assert_eq!(ui.screen, Screen::Room("living".into()));

		let v = ui.view(&home);
		assert_eq!(title(&v[0]), "All lights");
		assert_eq!(title(&v[1]), "Thermostat");

		// Tapping a thermostat opens it (there is nothing to toggle).
		assert!(tap(&mut ui, 1, &home).is_empty());
		assert_eq!(ui.screen, Screen::Entity { room: "living".into(), entity: "climate.living".into() });

		// Turn the big knob three clicks: 21 -> 22.5, shown at once, sent once the knob settles.
		ui.dial_rotate(0, 2, t0, &home);
		ui.dial_rotate(0, 1, t0 + Duration::from_millis(100), &home);
		match &ui.view(&home)[0] {
			KeyView::Tile(t) => assert_eq!(t.value, "22.5°"),
			other => panic!("{other:?}"),
		}
		let (_, calls) = ui.tick(t0 + Duration::from_millis(200), HOLD, None, &home);
		assert!(calls.is_empty(), "still turning");
		let (_, calls) = ui.tick(t0 + Duration::from_millis(500), HOLD, None, &home);
		assert_eq!(calls.len(), 1);
		assert_eq!(calls[0].service, "set_temperature");
		assert_eq!(calls[0].data["temperature"], json!(22.5));
		assert_eq!(calls[0].target["entity_id"], "climate.living");

		// Idle for the timeout: back to all rooms.
		let (redraw, _) = ui.tick(t0 + Duration::from_secs(31), HOLD, Some(Duration::from_secs(30)), &home);
		assert!(redraw);
		assert_eq!(ui.screen, Screen::Home);
	}

	#[test]
	fn small_knob_in_room_sets_the_room_thermostat() {
		let home = sample_home();
		let mut ui = s6();
		tap(&mut ui, 1, &home);
		assert_eq!(ui.screen, Screen::Room("bed".into()));
		let t0 = Instant::now();
		ui.dial_rotate(2, -2, t0, &home);
		let (_, calls) = ui.tick(t0 + Duration::from_secs(1), HOLD, None, &home);
		assert_eq!(calls[0].data["temperature"], json!(17.0));
	}

	#[test]
	fn tap_toggles_hold_opens() {
		let home = sample_home();
		let mut ui = s6();
		tap(&mut ui, 0, &home);
		let calls = tap(&mut ui, 3, &home); // Lamp (All lights, Thermostat, Ceiling, Lamp)
		assert_eq!(calls[0].service, "toggle");
		assert_eq!(calls[0].target["entity_id"], "light.living_room_lamp");

		let t0 = Instant::now();
		ui.key_down(3, t0);
		let (_, calls) = ui.tick(t0 + Duration::from_millis(600), HOLD, None, &home);
		assert!(calls.is_empty());
		assert_eq!(ui.screen, Screen::Entity { room: "living".into(), entity: "light.living_room_lamp".into() });
		// Releasing after the hold fired does nothing more.
		assert!(ui.key_up(3, t0 + Duration::from_millis(700), HOLD, &home).is_empty());

		// Back returns to the room with the lamp focused.
		tap(&mut ui, 6, &home);
		assert_eq!(ui.screen, Screen::Room("living".into()));
		assert!(matches!(&ui.view(&home)[3], KeyView::Tile(t) if t.focused));
	}

	#[test]
	fn all_lights_key_uses_area_target() {
		let home = sample_home();
		let mut ui = s6();
		tap(&mut ui, 0, &home);
		let calls = tap(&mut ui, 0, &home);
		assert_eq!(calls[0].domain, "light");
		assert_eq!(calls[0].service, "turn_off");
		assert_eq!(calls[0].target["area_id"], "living");
	}

	#[test]
	fn thermostat_knob_press_turns_off_and_back_to_previous_mode() {
		let mut home = sample_home();
		let mut ui = s6();
		tap(&mut ui, 0, &home);
		tap(&mut ui, 1, &home);
		let calls = ui.dial_press(0, Instant::now(), &home);
		assert_eq!(calls[0].data["hvac_mode"], "off");
		home.states.get_mut("climate.living").unwrap().state = "off".into();
		let calls = ui.dial_press(0, Instant::now(), &home);
		assert_eq!(calls[0].data["hvac_mode"], "heat");
	}

	#[test]
	fn deck_without_plain_keys_reserves_back_and_more() {
		let mut home = sample_home();
		// Fifteen lights in the living room.
		for i in 0..15 {
			let id = format!("light.extra_{i:02}");
			home.entities.insert(id.clone(), serde_json::from_value(json!({"entity_id": id, "area_id": "living"})).unwrap());
			home.states.insert(id.clone(), serde_json::from_value(json!({"entity_id": id, "state": "off", "attributes": {}})).unwrap());
		}
		home.rebuild_rooms(&Default::default());
		let mut ui = Ui::new(Layout::grid(3, 5, 0));
		tap(&mut ui, 0, &home);
		let v = ui.view(&home);
		assert_eq!(v[14], KeyView::Nav(Nav::Back, true));
		assert!(matches!(&v[13], KeyView::Tile(t) if t.title == "More" && t.value == "1/2"));
		tap(&mut ui, 13, &home);
		let v = ui.view(&home);
		assert!(matches!(&v[13], KeyView::Tile(t) if t.value == "2/2"));
		tap(&mut ui, 14, &home);
		assert_eq!(ui.screen, Screen::Home);
	}

	#[test]
	fn not_ready_shows_status() {
		let mut home = sample_home();
		home.status = Status::AuthFailed;
		let ui = s6();
		match &ui.view(&home)[0] {
			KeyView::Tile(t) => assert_eq!(t.value, "Token?"),
			other => panic!("{other:?}"),
		}
	}
}
