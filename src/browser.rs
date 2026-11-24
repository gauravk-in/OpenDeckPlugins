//! Opening links in a specific Chromium-family browser profile (Chrome, Brave, Edge, Chromium).
//!
//! Each browser keeps its profiles in a `Local State` JSON file; we list them so the user can
//! pick e.g. "Chrome: Work", then launch the browser with `--profile-directory=<dir> <url>`.
//! If that profile is already open, Chrome just adds a tab to its window.

use std::path::PathBuf;

use log::{info, warn};
use serde::Serialize;

struct Browser {
	id: &'static str,
	label: &'static str,
	/// Config folder relative to ~/Library/Application Support (macOS) or ~/.config (Linux).
	mac_dir: &'static str,
	linux_dir: &'static str,
	mac_app: &'static str,
	linux_bins: &'static [&'static str],
}

const BROWSERS: &[Browser] = &[
	Browser {
		id: "chrome",
		label: "Chrome",
		mac_dir: "Google/Chrome",
		linux_dir: "google-chrome",
		mac_app: "Google Chrome",
		linux_bins: &["google-chrome", "google-chrome-stable"],
	},
	Browser {
		id: "brave",
		label: "Brave",
		mac_dir: "BraveSoftware/Brave-Browser",
		linux_dir: "BraveSoftware/Brave-Browser",
		mac_app: "Brave Browser",
		linux_bins: &["brave-browser", "brave"],
	},
	Browser {
		id: "edge",
		label: "Edge",
		mac_dir: "Microsoft Edge",
		linux_dir: "microsoft-edge",
		mac_app: "Microsoft Edge",
		linux_bins: &["microsoft-edge", "microsoft-edge-stable"],
	},
	Browser {
		id: "chromium",
		label: "Chromium",
		mac_dir: "Chromium",
		linux_dir: "chromium",
		mac_app: "Chromium",
		linux_bins: &["chromium", "chromium-browser"],
	},
];

#[derive(Serialize, Clone, Debug)]
pub struct ProfileChoice {
	/// "<browser id>:<profile directory>", stored in the action settings.
	pub value: String,
	pub label: String,
}

fn config_root(b: &Browser) -> Option<PathBuf> {
	if cfg!(target_os = "macos") {
		Some(dirs::home_dir()?.join("Library/Application Support").join(b.mac_dir))
	} else {
		Some(dirs::config_dir()?.join(b.linux_dir))
	}
}

/// Lists the profiles of every installed Chromium-family browser.
pub fn list_profiles() -> Vec<ProfileChoice> {
	let mut out = Vec::new();
	for b in BROWSERS {
		let Some(root) = config_root(b) else { continue };
		let Ok(text) = std::fs::read_to_string(root.join("Local State")) else { continue };
		let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
		let Some(cache) = json.pointer("/profile/info_cache").and_then(|v| v.as_object()) else { continue };
		let mut profiles: Vec<_> = cache
			.iter()
			.map(|(dir, info)| {
				let name = info.get("name").and_then(|v| v.as_str()).unwrap_or(dir);
				let account = info.get("user_name").and_then(|v| v.as_str()).unwrap_or("");
				let label = if account.is_empty() { format!("{}: {name}", b.label) } else { format!("{}: {name} ({account})", b.label) };
				ProfileChoice { value: format!("{}:{dir}", b.id), label }
			})
			.collect();
		profiles.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
		out.extend(profiles);
	}
	out
}

fn find_on_path(bin: &str) -> Option<PathBuf> {
	std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|p| p.join(bin)).find(|p| p.is_file()))
}

/// Opens `url` in the profile named by `choice` ("chrome:Profile 1"). Returns false if that
/// browser could not be launched, so the caller can fall back to the default browser.
pub async fn open_in_profile(choice: &str, url: &str) -> bool {
	let Some((id, dir)) = choice.split_once(':') else { return false };
	let Some(b) = BROWSERS.iter().find(|b| b.id == id) else { return false };
	let profile_arg = format!("--profile-directory={dir}");

	let mut cmd = if cfg!(target_os = "macos") {
		let mut c = tokio::process::Command::new("/usr/bin/open");
		c.args(["-n", "-a", b.mac_app, "--args", &profile_arg, url]);
		c
	} else {
		let Some(bin) = b.linux_bins.iter().find_map(|bin| find_on_path(bin)) else {
			warn!("{} not found on PATH", b.label);
			return false;
		};
		let mut c = tokio::process::Command::new(bin);
		c.args([&profile_arg, url]);
		c
	};
	cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
	match cmd.spawn() {
		Ok(mut child) => {
			info!("opened {url} in {} profile {dir}", b.label);
			// Reap the launcher in the background (on macOS `open` exits right away; on Linux
			// the browser hands the URL to its running instance and exits).
			tokio::spawn(async move {
				let _ = child.wait().await;
			});
			true
		}
		Err(e) => {
			warn!("could not launch {}: {e}", b.label);
			false
		}
	}
}
