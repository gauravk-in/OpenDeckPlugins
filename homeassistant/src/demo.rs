//! A small made-up home, used by the tests and by `--preview` to draw example key images.

use serde_json::json;

use crate::model::*;
use crate::settings::GlobalSettings;

pub fn home() -> Home {
	let mut home = Home {
		status: Status::Ready,
		areas: serde_json::from_value(json!([
			{"area_id": "living", "name": "Living Room", "floor_id": "ground"},
			{"area_id": "bed", "name": "Bedroom", "floor_id": "first"},
			{"area_id": "garage", "name": "Garage", "floor_id": "ground"},
			{"area_id": "empty", "name": "Attic"}
		]))
		.unwrap(),
		floors: serde_json::from_value(json!([{"floor_id": "ground", "level": 0}, {"floor_id": "first", "level": 1}])).unwrap(),
		..Default::default()
	};
	let devices: Vec<DeviceReg> = serde_json::from_value(json!([
		{"id": "d1", "area_id": "living", "name": "Thermostat"},
		{"id": "d2", "area_id": "bed", "name": "Bedroom TRV"}
	]))
	.unwrap();
	home.devices = devices.into_iter().map(|d| (d.id.clone(), d)).collect();
	let entities: Vec<EntityReg> = serde_json::from_value(json!([
		{"entity_id": "climate.living", "device_id": "d1"},
		{"entity_id": "light.living_room_lamp", "area_id": "living"},
		{"entity_id": "light.living_room_ceiling", "area_id": "living"},
		{"entity_id": "cover.living_blinds", "area_id": "living"},
		{"entity_id": "sensor.living_temp", "area_id": "living"},
		{"entity_id": "sensor.living_battery", "area_id": "living", "entity_category": "diagnostic"},
		{"entity_id": "climate.bedroom", "device_id": "d2"},
		{"entity_id": "light.bedroom", "area_id": "bed"},
		{"entity_id": "switch.garage_door", "area_id": "garage", "hidden_by": "user"},
		{"entity_id": "light.no_area"}
	]))
	.unwrap();
	home.entities = entities.into_iter().map(|e| (e.entity_id.clone(), e)).collect();
	let states: Vec<EntityState> = serde_json::from_value(json!([
		{"entity_id": "climate.living", "state": "heat", "attributes": {"friendly_name": "Living Room Thermostat", "temperature": 21.0, "current_temperature": 20.4, "min_temp": 7, "max_temp": 30, "target_temp_step": 0.5, "hvac_modes": ["off", "heat", "auto"]}},
		{"entity_id": "light.living_room_lamp", "state": "on", "attributes": {"friendly_name": "Living Room Lamp", "brightness": 128, "supported_color_modes": ["color_temp"], "min_color_temp_kelvin": 2200, "max_color_temp_kelvin": 6500, "color_temp_kelvin": 3000}},
		{"entity_id": "light.living_room_ceiling", "state": "off", "attributes": {"friendly_name": "Living Room Ceiling", "supported_color_modes": ["brightness"]}},
		{"entity_id": "cover.living_blinds", "state": "open", "attributes": {"friendly_name": "Blinds", "current_position": 100, "supported_features": 15}},
		{"entity_id": "sensor.living_temp", "state": "20.6", "attributes": {"device_class": "temperature", "unit_of_measurement": "°C"}},
		{"entity_id": "climate.bedroom", "state": "off", "attributes": {"friendly_name": "Bedroom TRV", "temperature": 18.0, "current_temperature": 17.2, "hvac_modes": ["off", "heat"]}},
		{"entity_id": "light.bedroom", "state": "off", "attributes": {"friendly_name": "Bedroom Light"}}
	]))
	.unwrap();
	home.states = states.into_iter().map(|s| (s.entity_id.clone(), s)).collect();
	home.rebuild_rooms(&GlobalSettings::default());
	home
}
