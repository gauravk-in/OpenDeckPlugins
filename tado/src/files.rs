//! The plugin's own files: sign-in token, cached homes and rooms, and preferences.
//!
//! They live in the OS config folder (`~/Library/Application Support/opendeck-tado` on macOS,
//! `~/.config/opendeck-tado` on Linux). The token file is readable by the current user only.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

pub const AUTH: &str = "auth.json";
pub const HOMES: &str = "homes.json";
pub const PREFS: &str = "prefs.json";

pub fn dir() -> Option<PathBuf> {
	if let Ok(dir) = std::env::var("OPENDECK_TADO_DIR") {
		return Some(PathBuf::from(dir));
	}
	dirs::config_dir().map(|d| d.join("opendeck-tado"))
}

pub fn load<T: DeserializeOwned>(name: &str) -> Option<T> {
	let text = fs::read_to_string(dir()?.join(name)).ok()?;
	serde_json::from_str(&text).ok()
}

/// Writes a file atomically. With `private`, only the current user can read it.
pub fn save<T: Serialize>(name: &str, value: &T, private: bool) {
	if let Err(e) = try_save(name, value, private) {
		log::warn!("Could not save {name}: {e}");
	}
}

fn try_save<T: Serialize>(name: &str, value: &T, private: bool) -> std::io::Result<()> {
	let dir = dir().ok_or_else(|| std::io::Error::other("no config folder"))?;
	fs::create_dir_all(&dir)?;
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
	}
	let tmp = dir.join(format!(".{name}.tmp"));
	let mut options = fs::OpenOptions::new();
	options.write(true).create(true).truncate(true);
	#[cfg(unix)]
	if private {
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	#[cfg(not(unix))]
	let _ = private;
	let mut file = options.open(&tmp)?;
	file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
	file.sync_all()?;
	drop(file);
	fs::rename(tmp, dir.join(name))
}

pub fn remove(name: &str) {
	if let Some(dir) = dir() {
		let _ = fs::remove_file(dir.join(name));
	}
}
