//! Finds a device's OpenDeck profiles ("pages") and their order by reading OpenDeck's config folder.
//!
//! OpenDeck keeps each profile at `<config>/profiles/<device>/<id>.json` (or `<folder>/<id>.json`) and the
//! device's settings at `<config>/profiles/<device>.json`. Forks with custom ordering store the order there as
//! `profile_order`; without it, pages are alphabetical, matching stock OpenDeck.

use std::fs;
use std::path::{Path, PathBuf};

/// OpenDeck's config folder: the plugin runs from `<config>/plugins/<plugin>/...`.
pub fn config_dir() -> Option<PathBuf> {
	if let Ok(dir) = std::env::var("OPENDECK_CONFIG_DIR") {
		return Some(PathBuf::from(dir));
	}
	let exe = std::env::current_exe().ok()?;
	let exe = exe.canonicalize().unwrap_or(exe);
	exe.ancestors().find(|p| p.file_name().is_some_and(|n| n == "plugins")).and_then(Path::parent).map(Path::to_path_buf)
}

fn profile_id(name: &str) -> Option<&str> {
	[".json", ".json.bak", ".json.temp"].iter().find_map(|ext| name.strip_suffix(ext))
}

/// All profile IDs of a device, in the user's order (falling back to alphabetical).
pub fn device_pages(config: &Path, device: &str) -> Vec<String> {
	let mut pages = vec![];
	let dir = config.join("profiles").join(device);
	if let Ok(entries) = fs::read_dir(&dir) {
		for entry in entries.flatten() {
			let path = entry.path();
			let name = entry.file_name().to_string_lossy().into_owned();
			if path.is_file() {
				if let Some(id) = profile_id(&name) {
					pages.push(id.to_owned());
				}
			} else if path.is_dir()
				&& let Ok(sub) = fs::read_dir(&path)
			{
				for subentry in sub.flatten() {
					let subname = subentry.file_name().to_string_lossy().into_owned();
					if subentry.path().is_file()
						&& let Some(id) = profile_id(&subname)
					{
						pages.push(format!("{name}/{id}"));
					}
				}
			}
		}
	}
	if pages.is_empty() {
		pages.push("Default".to_owned());
	}

	let order = read_order(config, device);
	sort_pages(pages, &order)
}

fn read_order(config: &Path, device: &str) -> Vec<String> {
	let path = config.join("profiles").join(format!("{device}.json"));
	let Ok(text) = fs::read_to_string(path) else { return vec![] };
	let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
	value
		.get("profile_order")
		.and_then(|v| v.as_array())
		.map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
		.unwrap_or_default()
}

/// Same rule as the OpenDeck fork: listed pages first, in that order; the rest alphabetically after.
pub fn sort_pages(mut pages: Vec<String>, order: &[String]) -> Vec<String> {
	pages.sort();
	pages.dedup();
	pages.sort_by_key(|id| order.iter().position(|o| o == id).unwrap_or(usize::MAX));
	pages
}

fn folder(id: &str) -> &str {
	id.split_once('/').map(|(f, _)| f).unwrap_or("")
}

/// The page `steps` away from `current` (negative = backwards).
/// With `wrap`, going past either end continues from the other end; without it, it stops at the end.
/// With `same_folder`, only pages in the current page's folder are considered.
pub fn step(pages: &[String], current: &str, steps: i64, wrap: bool, same_folder: bool) -> Option<String> {
	let pool: Vec<&String> = if same_folder { pages.iter().filter(|p| folder(p) == folder(current)).collect() } else { pages.iter().collect() };
	if pool.is_empty() {
		return None;
	}
	let len = pool.len() as i64;
	let index = pool.iter().position(|p| *p == current).map(|i| i as i64).unwrap_or(if steps > 0 { -1 } else { len });
	let target = index + steps;
	let target = if wrap { target.rem_euclid(len) } else { target.clamp(0, len - 1) };
	let page = pool[target as usize];
	(page != current).then(|| page.clone())
}

/// Display name of a page: the part after the folder.
pub fn display_name(id: &str) -> &str {
	id.split_once('/').map(|(_, n)| n).unwrap_or(id)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn v(items: &[&str]) -> Vec<String> {
		items.iter().map(|s| s.to_string()).collect()
	}

	#[test]
	fn sorts_by_custom_order_then_alphabetically() {
		let pages = v(&["Lights", "Default", "Music", "Blinds", "Default"]);
		assert_eq!(sort_pages(pages.clone(), &[]), v(&["Blinds", "Default", "Lights", "Music"]));
		assert_eq!(sort_pages(pages, &v(&["Music", "Default"])), v(&["Music", "Default", "Blinds", "Lights"]));
	}

	#[test]
	fn steps_with_and_without_wrapping() {
		let pages = v(&["A", "B", "C"]);
		assert_eq!(step(&pages, "A", 1, true, false).as_deref(), Some("B"));
		assert_eq!(step(&pages, "C", 1, true, false).as_deref(), Some("A"));
		assert_eq!(step(&pages, "A", -1, true, false).as_deref(), Some("C"));
		assert_eq!(step(&pages, "C", 1, false, false), None);
		assert_eq!(step(&pages, "A", -3, false, false), None);
		assert_eq!(step(&pages, "A", 2, false, false).as_deref(), Some("C"));
		assert_eq!(step(&pages, "A", 5, false, false).as_deref(), Some("C"));
		assert_eq!(step(&v(&["A"]), "A", 1, true, false), None);
	}

	#[test]
	fn stays_in_folder_when_asked() {
		let pages = v(&["Home", "Rooms/Kitchen", "Rooms/Office", "Media"]);
		assert_eq!(step(&pages, "Rooms/Office", 1, true, true).as_deref(), Some("Rooms/Kitchen"));
		assert_eq!(step(&pages, "Rooms/Office", 1, true, false).as_deref(), Some("Media"));
		assert_eq!(step(&pages, "Home", 1, true, true).as_deref(), Some("Media"));
	}

	#[test]
	fn reads_profiles_and_order_from_disk() {
		let dir = std::env::temp_dir().join(format!("pageswitcher-test-{}", std::process::id()));
		let device = dir.join("profiles").join("dev1");
		fs::create_dir_all(device.join("Rooms")).unwrap();
		for f in ["Default.json", "Lights.json", "Lights.json.bak", "notes.txt", "Rooms/Kitchen.json"] {
			fs::write(device.join(f), "{}").unwrap();
		}
		assert_eq!(device_pages(&dir, "dev1"), v(&["Default", "Lights", "Rooms/Kitchen"]));
		fs::write(dir.join("profiles").join("dev1.json"), r#"{"selected_profile":"Default","profile_order":["Rooms/Kitchen","Lights"]}"#).unwrap();
		assert_eq!(device_pages(&dir, "dev1"), v(&["Rooms/Kitchen", "Lights", "Default"]));
		assert_eq!(device_pages(&dir, "missing"), v(&["Default"]));
		fs::remove_dir_all(dir).unwrap();
	}
}
