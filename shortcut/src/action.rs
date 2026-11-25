use std::sync::LazyLock;

use dashmap::DashMap;
use log::{info, warn};
use openaction::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::shortcut::{self, Chord, Shortcut};
use crate::{keys, render};

fn default_true() -> bool {
	true
}
fn default_mode() -> String {
	"tap".into()
}
fn default_repeat() -> u32 {
	1
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
	/// Pressed when the key (or the knob) is pressed, e.g. "cmd+shift+4" or "ctrl+k ctrl+s".
	pub shortcut: String,
	/// "tap": press and release right away. "hold": keep the combo down while the key is held.
	#[serde(default = "default_mode")]
	pub mode: String,
	/// Tap the shortcut this many times per press.
	#[serde(default = "default_repeat")]
	pub repeat: u32,
	/// Knobs: pressed once per step when turned right / left.
	pub clockwise: String,
	pub anticlockwise: String,
	/// Draw the shortcut on the key (older setting; `icon` wins when set).
	#[serde(default = "default_true")]
	pub show_shortcut: bool,
	/// What the key shows: "shortcut", "text", "image" or "none" (OpenDeck's own image and title).
	pub icon: String,
	/// Text for icon "text". Line breaks are kept.
	pub icon_text: String,
	/// Image for icon "image", as a data URL (the settings page scales it to 144x144).
	pub icon_image: String,
}

impl Settings {
	pub fn icon_mode(&self) -> &str {
		match self.icon.as_str() {
			"" if self.show_shortcut => "shortcut",
			"" => "none",
			m => m,
		}
	}
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			shortcut: String::new(),
			mode: default_mode(),
			repeat: default_repeat(),
			clockwise: String::new(),
			anticlockwise: String::new(),
			show_shortcut: true,
			icon: String::new(),
			icon_text: String::new(),
			icon_image: String::new(),
		}
	}
}

pub const MAC: bool = cfg!(target_os = "macos");

/// Combos held down right now, per instance, so they can be let go even if settings change meanwhile.
static HELD: LazyLock<DashMap<InstanceId, Chord>> = LazyLock::new(DashMap::new);
/// Settings each instance's image was last drawn for.
static DRAWN: LazyLock<DashMap<InstanceId, Settings>> = LazyLock::new(DashMap::new);

fn parsed(text: &str) -> Shortcut {
	shortcut::parse(text).unwrap_or_default()
}

/// What the key shows, one entry per line.
fn image_lines(instance: &Instance, s: &Settings) -> Vec<String> {
	let press = parsed(&s.shortcut).label(MAC);
	if instance.controller != "Encoder" {
		return if press.is_empty() { vec![] } else { vec![press] };
	}
	let (cw, acw) = (parsed(&s.clockwise).label(MAC), parsed(&s.anticlockwise).label(MAC));
	let mut lines = vec![];
	if !acw.is_empty() {
		lines.push(format!("↺ {acw}"));
	}
	if !cw.is_empty() {
		lines.push(format!("↻ {cw}"));
	}
	if !press.is_empty() {
		lines.push(if lines.is_empty() { press } else { format!("● {press}") });
	}
	lines
}

async fn draw(instance: &Instance, s: &Settings) {
	if DRAWN.get(&instance.instance_id).is_some_and(|d| *d == *s) {
		return;
	}
	DRAWN.insert(instance.instance_id.clone(), s.clone());
	let image = match s.icon_mode() {
		"text" if !s.icon_text.trim().is_empty() => Some(render::data_url(&render::to_png(&render::text_image(&s.icon_text)))),
		"image" if s.icon_image.starts_with("data:image/") => Some(s.icon_image.clone()),
		"shortcut" => {
			let lines = image_lines(instance, s);
			match lines.len() {
				0 => None,
				1 => Some(render::data_url(&render::to_png(&render::shortcut_image(&lines[0])))),
				_ => Some(render::data_url(&render::to_png(&render::lines_image(&lines)))),
			}
		}
		_ => None,
	};
	if let Err(e) = instance.set_image(image, None).await {
		warn!("set_image failed: {e}");
	}
}

fn field(text: &str) -> serde_json::Value {
	match shortcut::parse(text) {
		Ok(s) => json!({ "label": s.label(MAC), "canonical": s.canonical() }),
		Err(e) => json!({ "error": e }),
	}
}

async fn send_state(instance: &Instance, s: &Settings) {
	let status = match tokio::task::spawn_blocking(keys::ready).await.unwrap_or(Err("internal error".into())) {
		Ok(()) => json!({ "ok": true, "text": format!("Ready ({})", keys::backend()) }),
		Err(e) => json!({ "ok": false, "text": e }),
	};
	let _ = instance
		.send_to_property_inspector(json!({
			"platform": if MAC { "mac" } else { "linux" },
			"status": status,
			"fields": {
				"shortcut": field(&s.shortcut),
				"clockwise": field(&s.clockwise),
				"anticlockwise": field(&s.anticlockwise),
			},
		}))
		.await;
}

async fn report(instance: &Instance, result: Result<(), String>) {
	if let Err(e) = result {
		warn!("{e}");
		let _ = instance.show_alert().await;
		let _ = instance.send_to_property_inspector(json!({ "status": { "ok": false, "text": e } })).await;
	}
}

async fn tap(text: &str, times: u32) -> Result<(), String> {
	let s = shortcut::parse(text)?;
	if s.is_empty() {
		return Ok(());
	}
	info!("tap {} x{times}", s.canonical());
	tokio::task::spawn_blocking(move || keys::tap(&s, times)).await.map_err(|e| e.to_string())?
}

async fn press(instance: &Instance, s: &Settings) -> Result<(), String> {
	let shortcut = shortcut::parse(&s.shortcut)?;
	if s.mode == "hold" && shortcut.0.len() == 1 {
		let chord = shortcut.0[0].clone();
		if let Some((_, old)) = HELD.remove(&instance.instance_id) {
			tokio::task::spawn_blocking(move || keys::let_go(&old)).await.map_err(|e| e.to_string())??;
		}
		info!("hold {}", chord.canonical());
		HELD.insert(instance.instance_id.clone(), chord.clone());
		let result = tokio::task::spawn_blocking(move || keys::hold(&chord)).await.map_err(|e| e.to_string())?;
		if result.is_err() {
			release(instance).await?;
		}
		return result;
	}
	tap(&s.shortcut, s.repeat.clamp(1, 50)).await
}

async fn release(instance: &Instance) -> Result<(), String> {
	let Some((_, chord)) = HELD.remove(&instance.instance_id) else { return Ok(()) };
	info!("let go of {}", chord.canonical());
	tokio::task::spawn_blocking(move || keys::let_go(&chord)).await.map_err(|e| e.to_string())?
}

pub struct ShortcutAction;

#[async_trait]
impl Action for ShortcutAction {
	const UUID: &'static str = "io.github.gauravk-in.shortcut.press";
	type Settings = Settings;

	async fn will_appear(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		draw(instance, settings).await;
		// Sets up the virtual keyboard (Linux) or asks for Accessibility (macOS) before the first press.
		tokio::task::spawn_blocking(|| {
			if let Err(e) = keys::ready() {
				warn!("{e}");
			}
		});
		Ok(())
	}

	async fn will_disappear(&self, instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
		report(instance, release(instance).await).await;
		DRAWN.remove(&instance.instance_id);
		Ok(())
	}

	async fn did_receive_settings(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		draw(instance, settings).await;
		send_state(instance, settings).await;
		Ok(())
	}

	async fn property_inspector_did_appear(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		send_state(instance, settings).await;
		Ok(())
	}

	async fn send_to_plugin(&self, instance: &Instance, settings: &Self::Settings, payload: &serde_json::Value) -> OpenActionResult<()> {
		match payload.get("action").and_then(|a| a.as_str()) {
			Some("openAccessibility") => {
				#[cfg(target_os = "macos")]
				{
					keys::ask_for_access();
					let _ = tokio::process::Command::new("open").arg(keys::SETTINGS_URL).status().await;
				}
			}
			Some("check") => send_state(instance, settings).await,
			Some("test") => {
				let field = payload.get("field").and_then(|f| f.as_str()).unwrap_or("shortcut");
				let text = match field {
					"clockwise" => &settings.clockwise,
					"anticlockwise" => &settings.anticlockwise,
					_ => &settings.shortcut,
				};
				// Gives the user time to click into the app the shortcut is meant for.
				tokio::time::sleep(std::time::Duration::from_secs(3)).await;
				let result = tap(text, 1).await;
				let ok = result.is_ok();
				report(instance, result).await;
				if ok {
					let _ = instance.send_to_property_inspector(json!({ "tested": field })).await;
				}
			}
			_ => {}
		}
		Ok(())
	}

	async fn key_down(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		report(instance, press(instance, settings).await).await;
		Ok(())
	}

	async fn key_up(&self, instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
		report(instance, release(instance).await).await;
		Ok(())
	}

	async fn dial_down(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		report(instance, press(instance, settings).await).await;
		Ok(())
	}

	async fn dial_up(&self, instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
		report(instance, release(instance).await).await;
		Ok(())
	}

	async fn dial_rotate(&self, instance: &Instance, settings: &Self::Settings, ticks: i16, _pressed: bool) -> OpenActionResult<()> {
		let text = if ticks > 0 { &settings.clockwise } else { &settings.anticlockwise };
		report(instance, tap(text, ticks.unsigned_abs().min(20) as u32).await).await;
		Ok(())
	}
}
