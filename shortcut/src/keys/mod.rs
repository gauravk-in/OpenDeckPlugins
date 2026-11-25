//! Sends key presses to the operating system.

use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use crate::shortcut::{Chord, KeyDef, Shortcut};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as os;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod os {
	pub fn ready() -> Result<(), String> {
		Err("This plugin supports macOS and Linux only".into())
	}
	pub fn send(_: &crate::shortcut::KeyDef, _: bool, _: u64) -> Result<(), String> {
		ready()
	}
	pub fn backend() -> &'static str {
		"none"
	}
}

#[cfg(target_os = "macos")]
pub use mac::{SETTINGS_URL, ask_for_access};

/// Keys of one shortcut must not interleave with another shortcut's.
static LOCK: Mutex<()> = Mutex::new(());

/// Pause between key events. Some apps miss keys that arrive in the same instant as their modifiers.
const STEP: Duration = Duration::from_millis(8);
/// Pause between combos of a sequence such as `ctrl+k ctrl+s`.
const BETWEEN_CHORDS: Duration = Duration::from_millis(40);

/// For tests: with OPENDECK_SHORTCUT_DRY_RUN=<file>, key events are written to that file instead of sent.
static DRY_RUN: LazyLock<Option<String>> = LazyLock::new(|| std::env::var("OPENDECK_SHORTCUT_DRY_RUN").ok());

fn dry_run(path: &str, key: &KeyDef, down: bool, flags: u64) -> Result<(), String> {
	use std::io::Write;
	let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
	writeln!(f, "{} {} {flags:#x}", if down { "down" } else { "up" }, key.name).map_err(|e| e.to_string())
}

fn send(key: &KeyDef, down: bool, flags: u64) -> Result<(), String> {
	match DRY_RUN.as_deref() {
		Some(path) => dry_run(path, key, down, flags),
		None => os::send(key, down, flags),
	}
}

/// Ok when key presses can be sent, else what the user has to do.
pub fn ready() -> Result<(), String> {
	if DRY_RUN.is_some() {
		return Ok(());
	}
	os::ready()
}

pub fn backend() -> &'static str {
	if DRY_RUN.is_some() { "dry run" } else { os::backend() }
}

fn press(chord: &Chord) -> Result<(), String> {
	let mut flags = 0;
	for m in &chord.mods {
		flags |= m.mac_flag();
		send(m.key(), true, flags)?;
		std::thread::sleep(STEP);
	}
	if let Some(k) = chord.key {
		send(k, true, flags | k.mac_flags)?;
	}
	Ok(())
}

/// Releases in reverse order. Tries every key even if one fails, so nothing stays stuck.
fn release(chord: &Chord) -> Result<(), String> {
	let mut flags = chord.mac_flags();
	let mut result = Ok(());
	if let Some(k) = chord.key {
		result = result.and(send(k, false, flags | k.mac_flags));
	}
	for m in chord.mods.iter().rev() {
		std::thread::sleep(STEP);
		flags &= !m.mac_flag();
		result = result.and(send(m.key(), false, flags));
	}
	result
}

/// Presses and releases each combo of the shortcut, `times` times. Blocking.
pub fn tap(shortcut: &Shortcut, times: u32) -> Result<(), String> {
	ready()?;
	let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
	for i in 0..times {
		for (j, chord) in shortcut.0.iter().enumerate() {
			if i > 0 || j > 0 {
				std::thread::sleep(BETWEEN_CHORDS);
			}
			let pressed = press(chord);
			std::thread::sleep(STEP);
			let released = release(chord);
			pressed.and(released)?;
		}
	}
	Ok(())
}

/// Holds a single combo down (for push-to-talk and the like). Blocking.
pub fn hold(chord: &Chord) -> Result<(), String> {
	ready()?;
	let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
	press(chord)
}

/// Lets go of a combo pressed with [`hold`]. Blocking.
pub fn let_go(chord: &Chord) -> Result<(), String> {
	let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
	release(chord)
}
