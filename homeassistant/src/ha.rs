//! Home Assistant WebSocket API client: authenticates, loads the area/device/entity registries and states, keeps
//! states live, and sends service calls.
//!
//! <https://developers.home-assistant.io/docs/api/websocket>

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use crate::model::{EntityState, Home, Status};
use crate::settings::GlobalSettings;

/// What changed, so the deck can redraw only what it needs to.
#[derive(Clone, Debug)]
pub enum Change {
	/// Status, rooms or registries changed: redraw everything.
	All,
	Entity(String),
}

#[derive(Clone, Debug)]
pub struct ServiceCall {
	pub domain: String,
	pub service: String,
	pub data: Value,
	/// `{"entity_id": ...}` or `{"area_id": ...}`.
	pub target: Value,
}

impl ServiceCall {
	pub fn entity(entity_id: &str, service: &str, data: Value) -> Self {
		let domain = entity_id.split_once('.').map(|(d, _)| d).unwrap_or("homeassistant");
		Self { domain: domain.to_owned(), service: service.to_owned(), data, target: json!({ "entity_id": entity_id }) }
	}
	pub fn area(domain: &str, area_id: &str, service: &str, data: Value) -> Self {
		Self { domain: domain.to_owned(), service: service.to_owned(), data, target: json!({ "area_id": area_id }) }
	}
}

pub struct Client {
	calls: mpsc::UnboundedSender<ServiceCall>,
	task: JoinHandle<()>,
}

impl Client {
	pub fn call(&self, call: ServiceCall) {
		log::info!("call {}.{} {} {}", call.domain, call.service, call.target, call.data);
		let _ = self.calls.send(call);
	}
}

impl Drop for Client {
	fn drop(&mut self) {
		self.task.abort();
	}
}

fn set_status(home: &RwLock<Home>, changes: &broadcast::Sender<Change>, status: Status) {
	let mut h = home.write().unwrap();
	if h.status != status {
		h.status = status;
		drop(h);
		let _ = changes.send(Change::All);
	}
}

/// Starts a connection that reconnects with backoff until the client is dropped.
pub fn start(settings: GlobalSettings, home: Arc<RwLock<Home>>, changes: broadcast::Sender<Change>) -> Client {
	let (tx, mut rx) = mpsc::unbounded_channel();
	let task = tokio::spawn(async move {
		if !settings.configured() {
			set_status(&home, &changes, Status::NotConfigured);
			return;
		}
		let mut backoff = 1u64;
		loop {
			set_status(&home, &changes, Status::Connecting);
			match session(&settings, &home, &changes, &mut rx).await {
				Ok(()) => backoff = 1,
				Err(SessionError::Auth(msg)) => {
					log::error!("Home Assistant rejected the token: {msg}");
					set_status(&home, &changes, Status::AuthFailed);
					// The token will not start working by itself; wait for new settings, but retry rarely in
					// case HA was mid-startup.
					backoff = 300;
				}
				Err(SessionError::Other(msg)) => {
					log::warn!("Home Assistant connection lost: {msg}");
					set_status(&home, &changes, Status::Unreachable);
				}
			}
			tokio::time::sleep(Duration::from_secs(backoff)).await;
			backoff = (backoff * 2).min(300);
		}
	});
	Client { calls: tx, task }
}

enum SessionError {
	Auth(String),
	Other(String),
}

impl<E: std::fmt::Display> From<E> for SessionError {
	fn from(e: E) -> Self {
		SessionError::Other(e.to_string())
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
	Areas,
	Floors,
	Devices,
	Entities,
	States,
	Subscribe,
	Service,
}

const REGISTRY_EVENTS: [&str; 4] = ["area_registry_updated", "device_registry_updated", "entity_registry_updated", "floor_registry_updated"];

async fn session(
	settings: &GlobalSettings,
	home: &RwLock<Home>,
	changes: &broadcast::Sender<Change>,
	calls: &mut mpsc::UnboundedReceiver<ServiceCall>,
) -> Result<(), SessionError> {
	let url = settings.websocket_url().ok_or_else(|| SessionError::Other("no URL".into()))?;
	log::info!("Connecting to {url}");
	let (socket, _) = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(&url)).await??;
	let (mut write, mut read) = socket.split();

	// Authentication.
	loop {
		let msg = tokio::time::timeout(Duration::from_secs(15), read.next()).await?.ok_or("closed during auth")??;
		let Message::Text(text) = msg else { continue };
		let v: Value = serde_json::from_str(&text)?;
		match v["type"].as_str() {
			Some("auth_required") => {
				write.send(Message::text(json!({"type": "auth", "access_token": settings.token.trim()}).to_string())).await?;
			}
			Some("auth_ok") => break,
			Some("auth_invalid") => return Err(SessionError::Auth(v["message"].as_str().unwrap_or("").to_owned())),
			_ => {}
		}
	}

	let mut id = 0u64;
	let mut pending: HashMap<u64, Request> = HashMap::new();
	macro_rules! send {
		($req:expr, $body:expr) => {{
			id += 1;
			let mut body: Value = $body;
			body["id"] = json!(id);
			pending.insert(id, $req);
			write.send(Message::text(body.to_string())).await?;
		}};
	}

	let load_registries = [
		(Request::Floors, "config/floor_registry/list"),
		(Request::Areas, "config/area_registry/list"),
		(Request::Devices, "config/device_registry/list"),
		(Request::Entities, "config/entity_registry/list"),
	];
	for (req, ty) in load_registries {
		send!(req, json!({ "type": ty }));
	}
	send!(Request::States, json!({"type": "get_states"}));
	send!(Request::Subscribe, json!({"type": "subscribe_events", "event_type": "state_changed"}));
	for ev in REGISTRY_EVENTS {
		send!(Request::Subscribe, json!({"type": "subscribe_events", "event_type": ev}));
	}

	// Staged copy of the home while (re)loading, so the deck never shows half-loaded registries.
	let mut staged = Home::default();
	let mut loading: Vec<Request> = vec![Request::Floors, Request::Areas, Request::Devices, Request::Entities, Request::States];
	let mut ping = tokio::time::interval(Duration::from_secs(30));
	ping.tick().await;
	let mut awaiting_pong = false;
	let mut refresh_at: Option<tokio::time::Instant> = None;

	loop {
		let refresh_sleep = async {
			match refresh_at {
				Some(t) => tokio::time::sleep_until(t).await,
				None => std::future::pending().await,
			}
		};
		tokio::select! {
			msg = read.next() => {
				let msg = msg.ok_or("connection closed")??;
				let text = match msg {
					Message::Text(t) => t.to_string(),
					Message::Close(_) => return Err("closed by server".into()),
					_ => continue,
				};
				let v: Value = serde_json::from_str(&text)?;
				// HA may batch messages into one array when coalescing is on; it is off by default, but handle both.
				let items = match v { Value::Array(a) => a, other => vec![other] };
				for item in items {
					match item["type"].as_str() {
						Some("pong") => awaiting_pong = false,
						Some("result") => {
							let req = item["id"].as_u64().and_then(|i| pending.remove(&i));
							if item["success"] != json!(true) {
								log::warn!("HA request {:?} failed: {}", req, item["error"]);
								if let Some(r) = req { loading.retain(|x| *x != r); }
								continue;
							}
							let result = item["result"].clone();
							match req {
								Some(Request::Floors) => staged.floors = serde_json::from_value(result).unwrap_or_default(),
								Some(Request::Areas) => staged.areas = serde_json::from_value(result).unwrap_or_default(),
								Some(Request::Devices) => {
									let list: Vec<crate::model::DeviceReg> = serde_json::from_value(result).unwrap_or_default();
									staged.devices = list.into_iter().map(|d| (d.id.clone(), d)).collect();
								}
								Some(Request::Entities) => {
									let list: Vec<crate::model::EntityReg> = serde_json::from_value(result).unwrap_or_default();
									staged.entities = list.into_iter().map(|e| (e.entity_id.clone(), e)).collect();
								}
								Some(Request::States) => {
									let list: Vec<EntityState> = serde_json::from_value(result).unwrap_or_default();
									staged.states = list.into_iter().map(|s| (s.entity_id.clone(), s)).collect();
								}
								_ => {}
							}
							if let Some(r) = req && loading.contains(&r) {
								loading.retain(|x| *x != r);
								if loading.is_empty() {
									let mut h = home.write().unwrap();
									let mut fresh = std::mem::take(&mut staged);
									if fresh.states.is_empty() {
										// Registry-only refresh: keep the live states.
										fresh.states = std::mem::take(&mut h.states);
									}
									fresh.status = Status::Ready;
									fresh.rebuild_rooms(&crate::current_settings());
									log::info!("Loaded {} rooms, {} entities", fresh.rooms.len(), fresh.entities.len());
									*h = fresh;
									drop(h);
									let _ = changes.send(Change::All);
								}
							}
						}
						Some("event") => {
							let event = &item["event"];
							match event["event_type"].as_str() {
								Some("state_changed") => {
									let data = &event["data"];
									let Some(entity_id) = data["entity_id"].as_str() else { continue };
									let mut h = home.write().unwrap();
									match serde_json::from_value::<EntityState>(data["new_state"].clone()) {
										Ok(s) => { h.states.insert(entity_id.to_owned(), s); }
										Err(_) => { h.states.remove(entity_id); }
									}
									drop(h);
									let _ = changes.send(Change::Entity(entity_id.to_owned()));
								}
								Some(ev) if REGISTRY_EVENTS.contains(&ev) => {
									// Registry edits come in bursts (e.g. renaming many entities); reload once things settle.
									refresh_at = Some(tokio::time::Instant::now() + Duration::from_secs(2));
								}
								_ => {}
							}
						}
						_ => {}
					}
				}
			}
			_ = refresh_sleep => {
				refresh_at = None;
				if loading.is_empty() {
					staged = Home::default();
					loading = vec![Request::Floors, Request::Areas, Request::Devices, Request::Entities];
					for (req, ty) in load_registries {
						send!(req, json!({ "type": ty }));
					}
				} else {
					refresh_at = Some(tokio::time::Instant::now() + Duration::from_secs(2));
				}
			}
			call = calls.recv() => {
				let Some(call) = call else { return Ok(()) };
				send!(Request::Service, json!({
					"type": "call_service",
					"domain": call.domain,
					"service": call.service,
					"service_data": call.data,
					"target": call.target,
				}));
			}
			_ = ping.tick() => {
				if awaiting_pong {
					return Err("no pong from Home Assistant".into());
				}
				awaiting_pong = true;
				id += 1;
				write.send(Message::text(json!({"id": id, "type": "ping"}).to_string())).await?;
			}
		}
	}
}
