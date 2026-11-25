//! What the deck knows about the home: Home Assistant's areas, devices and entities, plus live entity states,
//! turned into an ordered list of rooms with the entities worth putting on a key.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::settings::GlobalSettings;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Area {
	pub area_id: String,
	pub name: String,
	#[serde(default)]
	pub floor_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Floor {
	pub floor_id: String,
	#[serde(default)]
	pub level: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct DeviceReg {
	pub id: String,
	#[serde(default)]
	pub area_id: Option<String>,
	#[serde(default)]
	pub disabled_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct EntityReg {
	pub entity_id: String,
	#[serde(default)]
	pub area_id: Option<String>,
	#[serde(default)]
	pub device_id: Option<String>,
	#[serde(default)]
	pub disabled_by: Option<String>,
	#[serde(default)]
	pub hidden_by: Option<String>,
	#[serde(default)]
	pub entity_category: Option<String>,
	#[serde(default)]
	pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EntityState {
	pub entity_id: String,
	pub state: String,
	#[serde(default)]
	pub attributes: Value,
}

impl EntityState {
	pub fn attr_f64(&self, key: &str) -> Option<f64> {
		self.attributes.get(key).and_then(Value::as_f64)
	}
	pub fn attr_str(&self, key: &str) -> Option<&str> {
		self.attributes.get(key).and_then(Value::as_str)
	}
	pub fn attr_list(&self, key: &str) -> Vec<String> {
		self.attributes
			.get(key)
			.and_then(Value::as_array)
			.map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
			.unwrap_or_default()
	}
	pub fn is_on(&self) -> bool {
		matches!(self.state.as_str(), "on" | "open" | "opening" | "playing" | "unlocked" | "heat" | "cool" | "heat_cool" | "auto" | "dry" | "fan_only")
	}
	pub fn available(&self) -> bool {
		!matches!(self.state.as_str(), "unavailable" | "unknown")
	}
}

/// The kinds of entity the deck can show and control, in the order they appear in a room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
	Climate,
	Light,
	Cover,
	Fan,
	MediaPlayer,
	Switch,
	Lock,
	Scene,
	Script,
}

impl Kind {
	pub fn of(entity_id: &str) -> Option<Kind> {
		Some(match entity_id.split_once('.')?.0 {
			"climate" => Kind::Climate,
			"light" => Kind::Light,
			"cover" => Kind::Cover,
			"fan" => Kind::Fan,
			"media_player" => Kind::MediaPlayer,
			"switch" | "input_boolean" => Kind::Switch,
			"lock" => Kind::Lock,
			"scene" => Kind::Scene,
			"script" => Kind::Script,
			_ => return None,
		})
	}

	/// Whether tapping the key should act right away (true) or open the detail screen (false).
	pub fn tap_acts(self) -> bool {
		!matches!(self, Kind::Climate | Kind::MediaPlayer)
	}

	/// Whether the entity has a detail screen worth opening.
	pub fn has_detail(self) -> bool {
		matches!(self, Kind::Climate | Kind::Light | Kind::Cover | Kind::Fan | Kind::MediaPlayer)
	}
}

#[derive(Clone, Debug, Default)]
pub struct Room {
	pub id: String,
	pub name: String,
	/// Controllable entities, sorted by kind then name.
	pub entities: Vec<String>,
	/// A temperature sensor (or thermostat) used for the room key's subtitle.
	pub temperature: Option<String>,
	pub humidity: Option<String>,
}

impl Room {
	pub fn lights(&self) -> impl Iterator<Item = &String> {
		self.entities.iter().filter(|e| e.starts_with("light."))
	}
	pub fn thermostat(&self) -> Option<&String> {
		self.entities.iter().find(|e| e.starts_with("climate."))
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
	#[default]
	NotConfigured,
	Connecting,
	AuthFailed,
	Unreachable,
	Ready,
}

/// Everything loaded from Home Assistant.
#[derive(Clone, Debug, Default)]
pub struct Home {
	pub status: Status,
	pub areas: Vec<Area>,
	pub floors: Vec<Floor>,
	pub devices: HashMap<String, DeviceReg>,
	pub entities: HashMap<String, EntityReg>,
	pub states: HashMap<String, EntityState>,
	pub rooms: Vec<Room>,
}

impl Home {
	pub fn state(&self, entity_id: &str) -> Option<&EntityState> {
		self.states.get(entity_id)
	}

	pub fn room(&self, id: &str) -> Option<&Room> {
		self.rooms.iter().find(|r| r.id == id)
	}

	pub fn room_of(&self, entity_id: &str) -> Option<&Room> {
		self.rooms.iter().find(|r| r.entities.iter().any(|e| e == entity_id))
	}

	/// Short display name: the friendly name without its room's name in front ("Living Room Lamp" -> "Lamp").
	pub fn display_name(&self, entity_id: &str, room: Option<&Room>) -> String {
		let full = self
			.entities
			.get(entity_id)
			.and_then(|e| e.name.clone())
			.or_else(|| self.states.get(entity_id).and_then(|s| s.attr_str("friendly_name").map(str::to_owned)))
			.unwrap_or_else(|| entity_id.split_once('.').map(|(_, n)| n.replace('_', " ")).unwrap_or_default());
		if let Some(room) = room {
			let lower = full.to_lowercase();
			let prefix = room.name.to_lowercase();
			if lower.starts_with(&prefix) && full.len() > room.name.len() {
				let rest = full[room.name.len()..].trim_start_matches([' ', '-', '_', ':']).trim();
				if !rest.is_empty() {
					return capitalise(rest);
				}
			}
		}
		full
	}

	fn area_of(&self, entity_id: &str) -> Option<String> {
		let reg = self.entities.get(entity_id)?;
		reg.area_id
			.clone()
			.or_else(|| reg.device_id.as_ref().and_then(|d| self.devices.get(d)).and_then(|d| d.area_id.clone()))
	}

	fn usable(&self, entity_id: &str, settings: &GlobalSettings) -> bool {
		if settings.hidden_entities.iter().any(|e| e == entity_id) {
			return false;
		}
		let Some(reg) = self.entities.get(entity_id) else { return false };
		if reg.disabled_by.is_some() || reg.hidden_by.is_some() || reg.entity_category.is_some() {
			return false;
		}
		if let Some(device) = reg.device_id.as_ref().and_then(|d| self.devices.get(d))
			&& device.disabled_by.is_some()
		{
			return false;
		}
		true
	}

	/// Rebuilds the room list from the registries. Areas are ordered by floor level, then name; areas with nothing
	/// to control or show are left out.
	pub fn rebuild_rooms(&mut self, settings: &GlobalSettings) {
		let mut by_area: HashMap<String, Room> = HashMap::new();
		let mut ids: Vec<&String> = self.entities.keys().collect();
		ids.sort();
		for id in ids {
			if !self.usable(id, settings) {
				continue;
			}
			let Some(area) = self.area_of(id) else { continue };
			let room = by_area.entry(area.clone()).or_default();
			if Kind::of(id).is_some() {
				room.entities.push(id.clone());
			} else if id.starts_with("sensor.")
				&& let Some(state) = self.states.get(id)
			{
				match state.attr_str("device_class") {
					Some("temperature") if room.temperature.is_none() => room.temperature = Some(id.clone()),
					Some("humidity") if room.humidity.is_none() => room.humidity = Some(id.clone()),
					_ => {}
				}
			}
		}

		let floor_level = |floor: &Option<String>| -> i64 {
			floor
				.as_ref()
				.and_then(|f| self.floors.iter().find(|x| &x.floor_id == f))
				.and_then(|f| f.level)
				.unwrap_or(i64::MAX)
		};
		let mut areas: Vec<&Area> = self.areas.iter().filter(|a| !settings.hidden_areas.contains(&a.area_id)).collect();
		areas.sort_by(|a, b| floor_level(&a.floor_id).cmp(&floor_level(&b.floor_id)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

		let mut rooms = vec![];
		for area in areas {
			let Some(mut room) = by_area.remove(&area.area_id) else { continue };
			if room.entities.is_empty() && room.temperature.is_none() {
				continue;
			}
			room.id = area.area_id.clone();
			room.name = area.name.clone();
			let names: HashMap<String, String> = room.entities.iter().map(|e| (e.clone(), self.display_name(e, None).to_lowercase())).collect();
			room.entities.sort_by(|a, b| Kind::of(a).cmp(&Kind::of(b)).then_with(|| names[a].cmp(&names[b])));
			if room.temperature.is_none() {
				room.temperature = room.thermostat().cloned();
			}
			rooms.push(room);
		}
		self.rooms = rooms;
	}

	/// The room key's subtitle: temperature, and how many lights are on.
	pub fn room_summary(&self, room: &Room) -> String {
		let mut parts = vec![];
		if let Some(t) = room.temperature.as_ref().and_then(|t| self.states.get(t)) {
			let value = if t.entity_id.starts_with("climate.") { t.attr_f64("current_temperature") } else { t.state.parse().ok() };
			if let Some(v) = value {
				parts.push(format!("{}°", fmt_num(v)));
			}
		}
		let on = room.lights().filter(|l| self.states.get(*l).is_some_and(EntityState::is_on)).count();
		if on > 0 {
			parts.push(format!("{on} on"));
		}
		parts.join(" · ")
	}
}

pub fn fmt_num(v: f64) -> String {
	if (v - v.round()).abs() < 0.05 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

fn capitalise(s: &str) -> String {
	let mut c = s.chars();
	match c.next() {
		Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
		None => String::new(),
	}
}

#[cfg(test)]
pub mod tests {
	use super::*;

	pub use crate::demo::home as sample_home;

	#[test]
	fn rooms_are_ordered_by_floor_and_skip_empty_areas() {
		let home = sample_home();
		let names: Vec<&str> = home.rooms.iter().map(|r| r.name.as_str()).collect();
		// Garage only has a hidden switch; Attic has nothing.
		assert_eq!(names, ["Living Room", "Bedroom"]);
	}

	#[test]
	fn entities_come_from_device_area_and_are_sorted_by_kind() {
		let home = sample_home();
		let living = home.room("living").unwrap();
		assert_eq!(living.entities, ["climate.living", "light.living_room_ceiling", "light.living_room_lamp", "cover.living_blinds"]);
		assert_eq!(living.temperature.as_deref(), Some("sensor.living_temp"));
		assert_eq!(home.room("bed").unwrap().temperature.as_deref(), Some("climate.bedroom"));
	}

	#[test]
	fn names_drop_the_room_prefix() {
		let home = sample_home();
		let living = home.room("living");
		assert_eq!(home.display_name("light.living_room_lamp", living), "Lamp");
		assert_eq!(home.display_name("cover.living_blinds", living), "Blinds");
		assert_eq!(home.room_summary(living.unwrap()), "20.6° · 1 on");
	}

	#[test]
	fn hidden_areas_and_entities_are_skipped() {
		let mut home = sample_home();
		let settings = GlobalSettings { hidden_areas: vec!["bed".into()], hidden_entities: vec!["cover.living_blinds".into()], ..Default::default() };
		home.rebuild_rooms(&settings);
		assert_eq!(home.rooms.len(), 1);
		assert!(!home.rooms[0].entities.contains(&"cover.living_blinds".to_owned()));
	}
}
