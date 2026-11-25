//! Plugin-wide settings, stored by OpenDeck as the plugin's global settings. The property inspector and the
//! mobile config app both edit these.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct GlobalSettings {
	/// Home Assistant base URL, e.g. `http://homeassistant.local:8123`.
	pub url: String,
	/// Long-lived access token (HA profile -> Security -> Long-lived access tokens).
	pub token: String,
	/// Seconds without input before the deck goes back to the room list. 0 turns this off.
	pub timeout_secs: u64,
	/// Area IDs left off the deck.
	pub hidden_areas: Vec<String>,
	/// Entity IDs left off the deck.
	pub hidden_entities: Vec<String>,
	/// Create the "Home Assistant" page on each connected deck when it does not exist yet.
	pub auto_page: bool,
	/// Seconds a key must be held to open a device's detail screen instead of toggling it.
	pub hold_secs: f64,
}

impl Default for GlobalSettings {
	fn default() -> Self {
		Self {
			url: String::new(),
			token: String::new(),
			timeout_secs: 30,
			hidden_areas: vec![],
			hidden_entities: vec![],
			auto_page: true,
			hold_secs: 0.5,
		}
	}
}

impl GlobalSettings {
	pub fn configured(&self) -> bool {
		!self.url.trim().is_empty() && !self.token.trim().is_empty()
	}

	/// The WebSocket API URL for the configured base URL.
	pub fn websocket_url(&self) -> Option<String> {
		let base = self.url.trim().trim_end_matches('/');
		let base = base.strip_suffix("/api/websocket").unwrap_or(base);
		let rest = if let Some(r) = base.strip_prefix("https://") {
			format!("wss://{r}")
		} else if let Some(r) = base.strip_prefix("http://") {
			format!("ws://{r}")
		} else if base.starts_with("ws://") || base.starts_with("wss://") {
			base.to_owned()
		} else if !base.is_empty() {
			format!("ws://{base}")
		} else {
			return None;
		};
		Some(format!("{rest}/api/websocket"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn websocket_url_from_base() {
		let s = |u: &str| GlobalSettings { url: u.into(), ..Default::default() }.websocket_url();
		assert_eq!(s("http://ha.local:8123").as_deref(), Some("ws://ha.local:8123/api/websocket"));
		assert_eq!(s("https://x.ui.nabu.casa/").as_deref(), Some("wss://x.ui.nabu.casa/api/websocket"));
		assert_eq!(s("192.168.1.5:8123").as_deref(), Some("ws://192.168.1.5:8123/api/websocket"));
		assert_eq!(s("ws://h/api/websocket").as_deref(), Some("ws://h/api/websocket"));
		assert_eq!(s(""), None);
	}

	#[test]
	fn missing_fields_take_defaults() {
		let s: GlobalSettings = serde_json::from_str(r#"{"url":"http://h"}"#).unwrap();
		assert_eq!(s.timeout_secs, 30);
		assert!(s.auto_page);
	}
}
