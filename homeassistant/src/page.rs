//! Creates the "Home Assistant" page on a deck: a profile with this plugin's action on every key and knob, so
//! the plugin can draw the whole deck itself.
//!
//! With Gaurav's OpenDeck fork and its remote API turned on (`<config>/remote.json`), the page is made through
//! that API, exactly as the desktop app would. Otherwise the profile file is written to disk; OpenDeck loads a
//! profile file the first time it is selected, so a brand-new page is picked up without a restart.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::layout::Layout;

pub const PAGE_NAME: &str = "Home Assistant";
pub const PLUGIN_UUID: &str = "io.github.gauravk-in.homeassistant";
pub const ACTION_UUID: &str = "io.github.gauravk-in.homeassistant.deck";

/// OpenDeck's config folder: the plugin runs from `<config>/plugins/<plugin>/...`.
pub fn config_dir() -> Option<PathBuf> {
	if let Ok(dir) = std::env::var("OPENDECK_CONFIG_DIR") {
		return Some(PathBuf::from(dir));
	}
	let exe = std::env::current_exe().ok()?;
	let exe = exe.canonicalize().unwrap_or(exe);
	exe.ancestors().find(|p| p.file_name().is_some_and(|n| n == "plugins")).and_then(Path::parent).map(Path::to_path_buf)
}

/// The plugin's folder name inside `<config>/plugins` (normally `<uuid>.sdPlugin`).
fn plugin_folder() -> String {
	std::env::current_exe()
		.ok()
		.and_then(|e| e.canonicalize().ok())
		.and_then(|e| e.ancestors().find(|p| p.extension().is_some_and(|x| x == "sdPlugin")).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()))
		.unwrap_or_else(|| format!("{PLUGIN_UUID}.sdPlugin"))
}

/// Number of knobs on a device, read from any page it already has (OpenDeck sizes `sliders` to the knob count).
pub fn encoders_from_pages(config: &Path, device: &str) -> Option<usize> {
	let dir = config.join("profiles").join(device);
	let mut stack = vec![dir];
	while let Some(d) = stack.pop() {
		for entry in fs::read_dir(&d).ok()?.flatten() {
			let path = entry.path();
			if path.is_dir() {
				stack.push(path);
			} else if path.extension().is_some_and(|e| e == "json")
				&& let Ok(text) = fs::read_to_string(&path)
				&& let Ok(v) = serde_json::from_str::<Value>(&text)
				&& let Some(s) = v.get("sliders").and_then(Value::as_array)
			{
				return Some(s.len());
			}
		}
	}
	None
}

fn state_path(config: &Path) -> PathBuf {
	config.join("settings").join(format!("{PLUGIN_UUID}.state.json"))
}

/// Devices the page was already made for, so a page the user deleted is not made again.
fn created(config: &Path) -> Vec<String> {
	fs::read_to_string(state_path(config)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| serde_json::from_value(v["createdPages"].clone()).ok()).unwrap_or_default()
}

fn mark_created(config: &Path, device: &str) {
	let mut list = created(config);
	if !list.iter().any(|d| d == device) {
		list.push(device.to_owned());
	}
	let _ = fs::create_dir_all(config.join("settings"));
	let _ = fs::write(state_path(config), json!({ "createdPages": list }).to_string());
}

/// The action as stored in a profile, with paths relative to the config folder.
fn action_json(folder: &str) -> Value {
	json!({
		"name": "Home Assistant Deck",
		"uuid": ACTION_UUID,
		"plugin": folder,
		"tooltip": "Runs this page as a Home Assistant remote",
		"icon": format!("plugins/{folder}/icons/action.png"),
		"disable_automatic_states": true,
		"visible_in_action_list": true,
		"supported_in_multi_actions": false,
		"property_inspector": format!("plugins/{folder}/propertyInspector/settings.html"),
		"controllers": ["Keypad", "Encoder"],
		"encoder": null,
		"states": [{ "image": format!("plugins/{folder}/icons/key.png"), "show": false }]
	})
}

pub fn profile_json(layout: &Layout, folder: &str) -> Value {
	let action = action_json(folder);
	let instance = |controller: &str, position: usize| {
		json!({
			"action": action,
			"context": format!("{controller}.{position}.0"),
			"states": action["states"],
			"current_state": 0,
			"settings": {},
			"children": null
		})
	};
	json!({
		"keys": (0..layout.keys).map(|p| instance("Keypad", p)).collect::<Vec<_>>(),
		"sliders": (0..layout.encoders).map(|p| instance("Encoder", p)).collect::<Vec<_>>(),
		"infobars": []
	})
}

pub enum Outcome {
	Exists,
	Created,
	Skipped,
}

/// Makes the page for `device` unless it exists or was made before. Returns whether to switch to it.
pub async fn ensure(device: &str, layout: &Layout) -> Outcome {
	let Some(config) = config_dir() else { return Outcome::Skipped };
	let path = config.join("profiles").join(device).join(format!("{PAGE_NAME}.json"));
	if path.exists() {
		return Outcome::Exists;
	}
	if created(&config).iter().any(|d| d == device) {
		return Outcome::Skipped;
	}
	let api = match remote_api(&config) {
		Some((port, token)) if api_authorised(port, &token).await => Some((port, token)),
		_ => None,
	};
	let made = match api {
		Some((port, token)) => match create_via_api(port, &token, device, layout).await {
			Ok(()) => true,
			Err(e) => {
				log::warn!("Remote API could not create the page ({e}); writing the file instead");
				write_file(&path, layout)
			}
		},
		None => write_file(&path, layout),
	};
	if made {
		mark_created(&config, device);
		log::info!("Created page \"{PAGE_NAME}\" for {device} ({})", layout.name);
		Outcome::Created
	} else {
		Outcome::Skipped
	}
}

fn write_file(path: &Path, layout: &Layout) -> bool {
	let Some(parent) = path.parent() else { return false };
	if let Err(e) = fs::create_dir_all(parent) {
		log::error!("Cannot create {}: {e}", parent.display());
		return false;
	}
	match fs::write(path, serde_json::to_vec_pretty(&profile_json(layout, &plugin_folder())).unwrap()) {
		Ok(()) => true,
		Err(e) => {
			log::error!("Cannot write {}: {e}", path.display());
			false
		}
	}
}

fn remote_api(config: &Path) -> Option<(u16, String)> {
	let v: Value = serde_json::from_str(&fs::read_to_string(config.join("remote.json")).ok()?).ok()?;
	// "enabled" may be false while the server runs (OpenDeck started with --remote), so whether the API is
	// usable is decided by asking it (`api_authorised`).
	let token = v.get("token")?.as_str()?.to_owned();
	let port = v.get("port").and_then(Value::as_u64).unwrap_or(57200) as u16;
	Some((port, token))
}

/// Whether the remote API is running and accepts the token (`GET /api/info` answers `"authorised": true`).
async fn api_authorised(port: u16, token: &str) -> bool {
	match http(port, token, "GET", "/api/info", "").await {
		Ok(v) => v.get("authorised") == Some(&json!(true)),
		Err(_) => false,
	}
}

async fn rpc(port: u16, token: &str, cmd: &str, args: Value) -> Result<Value, String> {
	let v = http(port, token, "POST", "/api/rpc", &json!({ "cmd": cmd, "args": args }).to_string()).await.map_err(|e| format!("{cmd}: {e}"))?;
	if let Some(e) = v.get("error") {
		return Err(format!("{cmd}: {e}"));
	}
	// Answers are `{ "ok": <result> }`.
	Ok(v.get("ok").cloned().unwrap_or(v))
}

/// Minimal HTTP/1.1 request to the fork's remote API on localhost (no HTTP client dependency needed).
async fn http(port: u16, token: &str, method: &str, path: &str, body: &str) -> Result<Value, String> {
	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	let mut stream = tokio::time::timeout(std::time::Duration::from_secs(3), tokio::net::TcpStream::connect(("127.0.0.1", port))).await.map_err(|_| "timeout".to_string())?.map_err(|e| e.to_string())?;
	let req = format!(
		"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
		body.len()
	);
	stream.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
	let mut buf = vec![];
	tokio::time::timeout(std::time::Duration::from_secs(10), stream.read_to_end(&mut buf)).await.map_err(|_| "timeout".to_string())?.map_err(|e| e.to_string())?;
	let text = String::from_utf8_lossy(&buf);
	let (head, body) = text.split_once("\r\n\r\n").ok_or("bad response")?;
	let status = head.split_whitespace().nth(1).unwrap_or("");
	if !status.starts_with('2') && status != "422" {
		return Err(format!("HTTP {status} {body}"));
	}
	let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(body) } else { body.to_owned() };
	Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
}

fn dechunk(body: &str) -> String {
	let mut out = String::new();
	let mut rest = body;
	while let Some((size, after)) = rest.split_once("\r\n") {
		let Ok(n) = usize::from_str_radix(size.trim(), 16) else { break };
		if n == 0 || after.len() < n {
			break;
		}
		out.push_str(&after[..n]);
		rest = after[n..].trim_start_matches("\r\n");
	}
	out
}

async fn create_via_api(port: u16, token: &str, device: &str, layout: &Layout) -> Result<(), String> {
	// Our action as OpenDeck knows it (with absolute paths), from the action list.
	let categories = rpc(port, token, "get_categories", json!({})).await?;
	let action = categories
		.as_object()
		.into_iter()
		.flat_map(|m| m.values())
		.filter_map(|c| c.get("actions").and_then(Value::as_array))
		.flatten()
		.find(|a| a.get("uuid").and_then(Value::as_str) == Some(ACTION_UUID))
		.cloned()
		.ok_or("action not in get_categories")?;
	// Selecting a page that does not exist creates it.
	rpc(port, token, "set_selected_profile", json!({ "device": device, "id": PAGE_NAME })).await?;
	let mut slots: Vec<(&str, usize)> = (0..layout.keys).map(|p| ("Keypad", p)).collect();
	slots.extend((0..layout.encoders).map(|p| ("Encoder", p)));
	for (controller, position) in slots {
		let context = json!({ "device": device, "profile": PAGE_NAME, "controller": controller, "position": position });
		rpc(port, token, "create_instance", json!({ "action": action, "context": context })).await?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn profile_has_action_on_every_key_and_knob() {
		let p = profile_json(&Layout::streonor_s6(), "x.sdPlugin");
		assert_eq!(p["keys"].as_array().unwrap().len(), 9);
		assert_eq!(p["sliders"].as_array().unwrap().len(), 3);
		assert_eq!(p["keys"][4]["context"], "Keypad.4.0");
		assert_eq!(p["sliders"][2]["context"], "Encoder.2.0");
		assert_eq!(p["keys"][0]["action"]["uuid"], ACTION_UUID);
		assert_eq!(p["keys"][0]["action"]["icon"], "plugins/x.sdPlugin/icons/action.png");
	}

	#[test]
	fn encoder_count_comes_from_existing_pages() {
		let dir = std::env::temp_dir().join(format!("ha-deck-test-{}", std::process::id()));
		let dev = dir.join("profiles").join("n3-ABC");
		fs::create_dir_all(dev.join("Folder")).unwrap();
		fs::write(dev.join("Folder").join("Default.json"), r#"{"keys":[],"sliders":[null,null,null]}"#).unwrap();
		assert_eq!(encoders_from_pages(&dir, "n3-ABC"), Some(3));
		assert_eq!(encoders_from_pages(&dir, "other"), None);
		fs::remove_dir_all(dir).unwrap();
	}

	/// Against the OpenDeck fork's mock remote server: `PORT=57299 node remote-ui/mock/server.mjs`, then
	/// `MOCK_REMOTE=57299:mocktoken0123456789:<device id> cargo test remote_api -- --nocapture`.
	#[tokio::test]
	async fn remote_api_creates_page() {
		let Ok(spec) = std::env::var("MOCK_REMOTE") else { return };
		let mut parts = spec.splitn(3, ':');
		let (port, token, device) = (parts.next().unwrap().parse().unwrap(), parts.next().unwrap(), parts.next().unwrap());
		assert!(api_authorised(port, token).await);
		assert!(!api_authorised(port, "wrong").await);
		assert!(!api_authorised(1, token).await);
		create_via_api(port, token, device, &Layout::streonor_s6()).await.unwrap();
		let profiles = rpc(port, token, "get_profiles", json!({ "device": device })).await.unwrap();
		assert!(profiles.to_string().contains(PAGE_NAME), "{profiles}");
		let selected = rpc(port, token, "get_selected_profile", json!({ "device": device })).await.unwrap();
		println!("{selected}");
		assert!(selected.to_string().contains(ACTION_UUID));
	}

	#[test]
	fn dechunks() {
		assert_eq!(dechunk("4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n"), "{\"a\":1}");
	}
}
