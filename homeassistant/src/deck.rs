//! Glue between OpenDeck and the UI: tracks which keys and knobs carry this plugin's action on each device,
//! forwards presses and turns to the UI, sends service calls to HA and redraws keys that changed.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

use openaction::global_events::*;
use openaction::*;
use tokio::sync::{Mutex, broadcast};

use crate::ha::{self, Change, ServiceCall};
use crate::layout::Layout;
use crate::model::Home;
use crate::page;
use crate::render;
use crate::settings::GlobalSettings;
use crate::ui::{KeyView, Ui};

struct Deck {
	ui: Ui,
	/// Key position -> instance on it.
	keys: HashMap<usize, InstanceId>,
	/// Knob index -> instance on it.
	knobs: HashMap<usize, InstanceId>,
	drawn: HashMap<usize, KeyView>,
}

pub struct App {
	pub settings: std::sync::RwLock<GlobalSettings>,
	home: Arc<RwLock<Home>>,
	changes: broadcast::Sender<Change>,
	client: Mutex<Option<ha::Client>>,
	decks: Mutex<HashMap<String, Deck>>,
	device_info: Mutex<HashMap<String, (String, u8, u8, u8)>>,
}

pub static APP: LazyLock<App> = LazyLock::new(|| App {
	settings: std::sync::RwLock::new(GlobalSettings::default()),
	home: Arc::new(RwLock::new(Home::default())),
	changes: broadcast::channel(256).0,
	client: Mutex::new(None),
	decks: Mutex::new(HashMap::new()),
	device_info: Mutex::new(HashMap::new()),
});

fn settings() -> GlobalSettings {
	APP.settings.read().unwrap().clone()
}

fn hold() -> Duration {
	Duration::from_secs_f64(settings().hold_secs.clamp(0.2, 3.0))
}

fn timeout() -> Option<Duration> {
	let s = settings().timeout_secs;
	(s > 0).then(|| Duration::from_secs(s))
}

async fn layout_for(device: &str) -> Layout {
	let info = APP.device_info.lock().await.get(device).cloned();
	let (name, rows, cols, ty) = match info {
		Some(i) => i,
		None => match get_connected_devices().await.get(device) {
			Some(d) => (d.name.clone(), d.size.rows, d.size.columns, d.r#type),
			None => (device.to_owned(), 3, 3, 0),
		},
	};
	let encoders = page::config_dir().and_then(|c| page::encoders_from_pages(&c, device));
	Layout::for_device(&name, rows, cols, ty, encoders)
}

/// Position of an instance: key index (row * columns + column) or knob index.
fn position(instance: &Instance, columns: usize) -> Option<usize> {
	// Context is `device.profile.controller.position.index`; the position is authoritative.
	let parts: Vec<&str> = instance.instance_id.rsplitn(3, '.').collect();
	if let Some(p) = parts.get(1).and_then(|p| p.parse().ok()) {
		return Some(p);
	}
	let c = instance.coordinates?;
	Some(if instance.controller == "Encoder" { c.column as usize } else { c.row as usize * columns + c.column as usize })
}

async fn send_calls(calls: Vec<ServiceCall>) {
	if calls.is_empty() {
		return;
	}
	if let Some(client) = APP.client.lock().await.as_ref() {
		for c in calls {
			client.call(c);
		}
	}
}

/// Redraws the keys of `device` whose picture changed.
async fn redraw(device: &str) {
	let home = APP.home.read().unwrap().clone();
	let mut decks = APP.decks.lock().await;
	let Some(deck) = decks.get_mut(device) else { return };
	deck.ui.validate(&home);
	let view = deck.ui.view(&home);
	let mut updates = vec![];
	for (pos, id) in &deck.keys {
		let Some(v) = view.get(*pos) else { continue };
		if !deck.ui.layout.display.get(*pos).copied().unwrap_or(true) {
			continue;
		}
		if deck.drawn.get(pos) == Some(v) {
			continue;
		}
		deck.drawn.insert(*pos, v.clone());
		updates.push((id.clone(), v.clone()));
	}
	drop(decks);
	for (id, v) in updates {
		let url = tokio::task::spawn_blocking(move || render::data_url(&render::key(&v))).await.unwrap_or_default();
		if let Some(instance) = get_instance(id).await {
			let _ = instance.set_image(Some(url), None).await;
		}
	}
}

async fn redraw_all() {
	let devices: Vec<String> = APP.decks.lock().await.keys().cloned().collect();
	for d in devices {
		redraw(&d).await;
	}
}

/// (Re)connects to HA with the current settings.
async fn connect() {
	let s = settings();
	let mut client = APP.client.lock().await;
	*client = None;
	*client = Some(ha::start(s, APP.home.clone(), APP.changes.clone()));
}

/// Background loops: HA changes -> redraw, and the UI clock (knob settling, holds, idle timeout).
pub fn spawn_loops() {
	let mut rx = APP.changes.subscribe();
	tokio::spawn(async move {
		loop {
			match rx.recv().await {
				Ok(Change::Entity(id)) => {
					let home = APP.home.read().unwrap().clone();
					for deck in APP.decks.lock().await.values_mut() {
						deck.ui.state_changed(&id, &home);
					}
					redraw_all().await;
				}
				Ok(Change::All) | Err(broadcast::error::RecvError::Lagged(_)) => redraw_all().await,
				Err(broadcast::error::RecvError::Closed) => break,
			}
		}
	});
	tokio::spawn(async move {
		let mut interval = tokio::time::interval(Duration::from_millis(100));
		loop {
			interval.tick().await;
			let home = APP.home.read().unwrap().clone();
			let (hold, timeout) = (hold(), timeout());
			let mut dirty = vec![];
			let mut calls = vec![];
			for (device, deck) in APP.decks.lock().await.iter_mut() {
				let (redraw, c) = deck.ui.tick(Instant::now(), hold, timeout, &home);
				if redraw || !c.is_empty() {
					dirty.push(device.clone());
				}
				calls.extend(c);
			}
			send_calls(calls).await;
			for d in dirty {
				redraw(&d).await;
			}
		}
	});
}

pub struct DeckAction;

impl DeckAction {
	async fn with_ui<F: FnOnce(&mut Ui, &Home, usize) -> Vec<ServiceCall>>(instance: &Instance, f: F) {
		let home = APP.home.read().unwrap().clone();
		let calls = {
			let mut decks = APP.decks.lock().await;
			let Some(deck) = decks.get_mut(&instance.device_id) else { return };
			let Some(pos) = position(instance, deck.ui.layout.columns) else { return };
			f(&mut deck.ui, &home, pos)
		};
		send_calls(calls).await;
		redraw(&instance.device_id).await;
	}
}

#[async_trait]
impl Action for DeckAction {
	const UUID: &'static str = page::ACTION_UUID;
	type Settings = serde_json::Value;

	async fn will_appear(&self, instance: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
		if instance.is_in_multi_action {
			return Ok(());
		}
		let layout = layout_for(&instance.device_id).await;
		{
			let mut decks = APP.decks.lock().await;
			let deck = decks.entry(instance.device_id.clone()).or_insert_with(|| {
				let mut l = layout;
				l.owned = vec![false; l.keys];
				log::info!("Deck {}: {} ({} keys, {} knobs)", instance.device_id, l.name, l.keys, l.encoders);
				Deck { ui: Ui::new(l), keys: HashMap::new(), knobs: HashMap::new(), drawn: HashMap::new() }
			});
			let Some(pos) = position(instance, deck.ui.layout.columns) else { return Ok(()) };
			if instance.controller == "Encoder" {
				deck.knobs.insert(pos, instance.instance_id.clone());
				deck.ui.layout.encoders = deck.ui.layout.encoders.max(pos + 1);
			} else if pos < deck.ui.layout.keys {
				deck.keys.insert(pos, instance.instance_id.clone());
				deck.ui.layout.owned[pos] = true;
				// Every key's picture depends on how many keys are ours, so draw them all again.
				deck.drawn.clear();
			}
		}
		// Keys appear one by one when a page opens; draw once they have all arrived.
		let device = instance.device_id.clone();
		tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(150)).await;
			redraw(&device).await;
		});
		Ok(())
	}

	async fn will_disappear(&self, instance: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
		let mut decks = APP.decks.lock().await;
		let Some(deck) = decks.get_mut(&instance.device_id) else { return Ok(()) };
		let Some(pos) = position(instance, deck.ui.layout.columns) else { return Ok(()) };
		if instance.controller == "Encoder" {
			deck.knobs.remove(&pos);
		} else {
			deck.keys.remove(&pos);
			if let Some(o) = deck.ui.layout.owned.get_mut(pos) {
				*o = false;
			}
			deck.drawn.clear();
		}
		if deck.keys.is_empty() && deck.knobs.is_empty() {
			// Page left: start from the room list next time.
			decks.remove(&instance.device_id);
		}
		Ok(())
	}

	async fn key_down(&self, instance: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
		Self::with_ui(instance, |ui, _, pos| {
			ui.key_down(pos, Instant::now());
			vec![]
		})
		.await;
		Ok(())
	}

	async fn key_up(&self, instance: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
		let hold = hold();
		Self::with_ui(instance, |ui, home, pos| ui.key_up(pos, Instant::now(), hold, home)).await;
		Ok(())
	}

	async fn dial_rotate(&self, instance: &Instance, _: &Self::Settings, ticks: i16, _pressed: bool) -> OpenActionResult<()> {
		Self::with_ui(instance, |ui, home, pos| ui.dial_rotate(pos, ticks, Instant::now(), home)).await;
		Ok(())
	}

	async fn dial_up(&self, instance: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
		Self::with_ui(instance, |ui, home, pos| ui.dial_press(pos, Instant::now(), home)).await;
		Ok(())
	}

	async fn property_inspector_did_appear(&self, instance: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
		send_status(instance).await
	}

	async fn send_to_plugin(&self, instance: &Instance, _: &Self::Settings, payload: &serde_json::Value) -> OpenActionResult<()> {
		match payload.get("event").and_then(|e| e.as_str()) {
			Some("status") => send_status(instance).await?,
			Some("reconnect") => connect().await,
			Some("createPage") => {
				let layout = layout_for(&instance.device_id).await;
				if let page::Outcome::Created = page::ensure(&instance.device_id, &layout).await {
					switch_to_page(&instance.device_id).await;
				}
			}
			_ => {}
		}
		Ok(())
	}
}

/// Tells the property inspector how the connection is doing and what was found.
async fn send_status(instance: &Instance) -> OpenActionResult<()> {
	let (status, rooms, areas) = {
		let h = APP.home.read().unwrap();
		(format!("{:?}", h.status), h.rooms.iter().map(|r| serde_json::json!({"id": r.id, "name": r.name, "entities": r.entities.len()})).collect::<Vec<_>>(), h.areas.iter().map(|a| serde_json::json!({"id": a.area_id, "name": a.name})).collect::<Vec<_>>())
	};
	let layout = APP.decks.lock().await.get(&instance.device_id).map(|d| d.ui.layout.name.clone()).unwrap_or_default();
	instance.send_to_property_inspector(serde_json::json!({ "event": "status", "status": status, "rooms": rooms, "areas": areas, "deck": layout })).await
}

#[derive(serde::Serialize)]
struct SwitchProfileEvent<'a> {
	event: &'static str,
	device: &'a str,
	profile: &'a str,
}

async fn switch_to_page(device: &str) {
	let _ = send_arbitrary_json(SwitchProfileEvent { event: "switchProfile", device, profile: page::PAGE_NAME }).await;
}

pub struct Events;

#[async_trait]
impl GlobalEventHandler for Events {
	async fn plugin_ready(&self) -> OpenActionResult<()> {
		get_global_settings().await?;
		// Devices that were connected before the plugin started.
		for (id, d) in get_connected_devices().await {
			APP.device_info.lock().await.insert(id.clone(), (d.name.clone(), d.size.rows, d.size.columns, d.r#type));
		}
		Ok(())
	}

	async fn did_receive_global_settings(&self, event: DidReceiveGlobalSettingsEvent) -> OpenActionResult<()> {
		let new: GlobalSettings = serde_json::from_value(event.payload.settings.clone()).unwrap_or_default();
		let old = std::mem::replace(&mut *APP.settings.write().unwrap(), new.clone());
		let first = APP.client.lock().await.is_none();
		if first || old.url != new.url || old.token != new.token {
			connect().await;
		} else if old.hidden_areas != new.hidden_areas || old.hidden_entities != new.hidden_entities {
			APP.home.write().unwrap().rebuild_rooms(&new);
			let _ = APP.changes.send(Change::All);
		}
		if first && new.auto_page {
			for device in APP.device_info.lock().await.keys().cloned().collect::<Vec<_>>() {
				auto_page(device);
			}
		}
		Ok(())
	}

	async fn device_did_connect(&self, event: DeviceDidConnectEvent) -> OpenActionResult<()> {
		let d = &event.deviceInfo;
		APP.device_info.lock().await.insert(event.device.clone(), (d.name.clone(), d.size.rows, d.size.columns, d.r#type));
		// Settings arrive shortly after start; wait for them before deciding on auto_page.
		if APP.client.lock().await.is_some() && settings().auto_page {
			auto_page(event.device.clone());
		}
		Ok(())
	}
}

fn auto_page(device: String) {
	tokio::spawn(async move {
		// Give OpenDeck a moment to finish registering the device (and its existing pages).
		tokio::time::sleep(Duration::from_secs(2)).await;
		let layout = layout_for(&device).await;
		if let page::Outcome::Created = page::ensure(&device, &layout).await {
			switch_to_page(&device).await;
		}
	});
}
