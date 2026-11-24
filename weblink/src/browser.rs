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

/// Reduces a URL to what matters for "is this the same page": no scheme, no "www.",
/// no fragment, no trailing slash. "https://www.github.com/foo/#x" -> "github.com/foo".
pub fn match_key(url: &str) -> String {
	let s = url.trim();
	let s = s.split_once("://").map_or(s, |(_, rest)| rest);
	let s = s.strip_prefix("www.").unwrap_or(s);
	let s = s.split('#').next().unwrap_or(s);
	s.trim_end_matches('/').to_ascii_lowercase()
}

/// True when an open tab shows the key's page or something below it.
/// "github.com/foo" matches "github.com/foo", "github.com/foo/pulls" and "github.com/foo?tab=x",
/// but not "github.com/foobar".
pub fn tab_matches(tab_url: &str, key_url: &str) -> bool {
	let (tab, key) = (match_key(tab_url), match_key(key_url));
	if key.is_empty() {
		return false;
	}
	tab == key || (tab.starts_with(&key) && matches!(tab.as_bytes()[key.len()], b'/' | b'?'))
}

async fn osascript(script: &str, args: &[&str]) -> Option<String> {
	let out = tokio::process::Command::new("/usr/bin/osascript")
		.arg("-e")
		.arg(script)
		.args(args)
		.stdin(std::process::Stdio::null())
		.output()
		.await
		.ok()?;
	if !out.status.success() {
		warn!("osascript failed: {}", String::from_utf8_lossy(&out.stderr).trim());
		return None;
	}
	Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Lists "window<TAB>tab<TAB>url" lines for every open tab, without launching the app.
const CHROME_LIST: &str = r#"on run argv
	set appName to item 1 of argv
	if application appName is not running then return ""
	set out to ""
	set TB to character id 9
	using terms from application "Google Chrome"
		tell application appName
			set wi to 0
			repeat with w in windows
				set wi to wi + 1
				set ti to 0
				try
					repeat with u in (URL of tabs of w)
						set ti to ti + 1
						set out to out & (wi as text) & TB & (ti as text) & TB & ((contents of u) as text) & linefeed
					end repeat
				end try
			end repeat
		end tell
	end using terms from
	return out
end run"#;

const CHROME_FOCUS: &str = r#"on run argv
	set appName to item 1 of argv
	set wi to (item 2 of argv) as integer
	set ti to (item 3 of argv) as integer
	using terms from application "Google Chrome"
		tell application appName
			set w to window wi
			set active tab index of w to ti
			set index of w to 1
			activate
		end tell
	end using terms from
end run"#;

const SAFARI_LIST: &str = r#"on run argv
	if application "Safari" is not running then return ""
	set out to ""
	set TB to character id 9
	tell application "Safari"
		set wi to 0
		repeat with w in windows
			set wi to wi + 1
			set ti to 0
			try
				repeat with u in (URL of tabs of w)
					set ti to ti + 1
					set out to out & (wi as text) & TB & (ti as text) & TB & ((contents of u) as text) & linefeed
				end repeat
			end try
		end repeat
	end tell
	return out
end run"#;

const SAFARI_FOCUS: &str = r#"on run argv
	set wi to (item 2 of argv) as integer
	set ti to (item 3 of argv) as integer
	tell application "Safari"
		set w to window wi
		set current tab of w to tab ti of w
		set index of w to 1
		activate
	end tell
end run"#;

/// macOS only: if a tab showing `url` is already open, bring it to the front and return true.
/// `choice` is the "Open with" setting; with the default browser we look in Safari and Chrome
/// (only if they are already running). Chrome's scripting can't tell profiles apart, so with
/// several profiles the first matching tab in any profile wins.
pub async fn focus_existing_tab(choice: &str, url: &str) -> bool {
	if !cfg!(target_os = "macos") {
		return false;
	}
	let apps: Vec<(&str, bool)> = match choice.split_once(':').and_then(|(id, _)| BROWSERS.iter().find(|b| b.id == id)) {
		Some(b) => vec![(b.mac_app, false)],
		None => vec![("Safari", true), ("Google Chrome", false)],
	};
	for (app, safari) in apps {
		let list = if safari { osascript(SAFARI_LIST, &[app]).await } else { osascript(CHROME_LIST, &[app]).await };
		let Some(list) = list else { continue };
		let hit = list.lines().find_map(|line| {
			let mut parts = line.splitn(3, '\t');
			let (w, t, u) = (parts.next()?, parts.next()?, parts.next()?);
			tab_matches(u, url).then(|| (w.to_owned(), t.to_owned()))
		});
		if let Some((w, t)) = hit {
			let script = if safari { SAFARI_FOCUS } else { CHROME_FOCUS };
			if osascript(script, &[app, &w, &t]).await.is_some() {
				info!("focused existing {app} tab for {url}");
				return true;
			}
		}
	}
	false
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn matching() {
		assert!(tab_matches("https://www.github.com/", "github.com"));
		assert!(tab_matches("https://github.com/foo/pulls", "https://github.com/foo"));
		assert!(tab_matches("https://github.com/foo?tab=x#y", "https://github.com/foo/"));
		assert!(!tab_matches("https://github.com/foobar", "https://github.com/foo"));
		assert!(!tab_matches("https://gitlab.com/", "github.com"));
		assert!(tab_matches("http://homeassistant.local:8123/lovelace/0", "https://homeassistant.local:8123"));
	}
}
