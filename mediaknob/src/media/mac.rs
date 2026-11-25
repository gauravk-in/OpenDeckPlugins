use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use super::run::{Output, run};
use super::{Level, Player};

// ---------- Volume (AppleScript) ----------

async fn osascript(script: &str) -> Option<String> {
	match run("osascript", &["-e", script]).await {
		Output::Ok(out) => Some(out.trim().to_owned()),
		Output::Missing => None,
		Output::Failed(e) => {
			log::warn!("osascript failed: {e}");
			None
		}
	}
}

const REPORT: &str = r#"set s to get volume settings
return ((output volume of s) as text) & "," & ((output muted of s) as text)"#;

/// Parses "45,false". The volume is "missing value" for outputs macOS cannot set (some HDMI/USB devices).
fn parse_report(out: &str) -> Option<Level> {
	let (volume, muted) = out.split_once(',')?;
	Some(Level { percent: volume.trim().parse().ok()?, muted: muted.trim() == "true" })
}

pub async fn level() -> Option<Level> {
	parse_report(&osascript(REPORT).await?)
}

pub async fn change_volume(delta: i32) -> Option<Level> {
	let unmute = if delta > 0 { "set volume output muted false\n" } else { "" };
	let script = format!(
		"set v to (output volume of (get volume settings)) + ({delta})
if v < 0 then set v to 0
if v > 100 then set v to 100
set volume output volume v
{unmute}{REPORT}"
	);
	parse_report(&osascript(&script).await?)
}

pub async fn toggle_mute() -> Option<Level> {
	let script = format!("set volume output muted (not (output muted of (get volume settings)))\n{REPORT}");
	parse_report(&osascript(&script).await?)
}

// ---------- Media keys ----------
//
// Play/pause, next and previous are sent as the keyboard's media keys, so they reach whatever app macOS
// shows in Now Playing (Music, Spotify, Safari, Chrome, ...). Sending keys needs OpenDeck to be allowed under
// System Settings > Privacy & Security > Accessibility.

const NX_KEYTYPE_PLAY: i64 = 16;
const NX_KEYTYPE_NEXT: i64 = 17;
const NX_KEYTYPE_PREVIOUS: i64 = 18;

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
	x: f64,
	y: f64,
}

type Id = *mut c_void;
type Sel = *mut c_void;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
	fn CGEventPost(tap: u32, event: *mut c_void);
	fn AXIsProcessTrusted() -> bool;
	fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
	fn CGPreflightPostEventAccess() -> bool;
	fn CGRequestPostEventAccess() -> bool;
	static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	static kCFBooleanTrue: *const c_void;
	static kCFTypeDictionaryKeyCallBacks: c_void;
	static kCFTypeDictionaryValueCallBacks: c_void;
	fn CFDictionaryCreate(
		allocator: *const c_void,
		keys: *const *const c_void,
		values: *const *const c_void,
		count: isize,
		key_callbacks: *const c_void,
		value_callbacks: *const c_void,
	) -> *const c_void;
	fn CFRelease(cf: *const c_void);
}

/// Asks macOS to show its "OpenDeck would like to control this computer" prompt, which also adds the running
/// OpenDeck app to the Accessibility list. Without it, a differently built OpenDeck (another bundle id or
/// signature) never shows up there, and the switch the user turned on belongs to the other build.
fn ask_for_accessibility() -> bool {
	unsafe {
		let keys = [kAXTrustedCheckOptionPrompt];
		let values = [kCFBooleanTrue];
		let options = CFDictionaryCreate(
			std::ptr::null(),
			keys.as_ptr(),
			values.as_ptr(),
			1,
			&raw const kCFTypeDictionaryKeyCallBacks,
			&raw const kCFTypeDictionaryValueCallBacks,
		);
		let trusted = AXIsProcessTrustedWithOptions(options);
		if !options.is_null() {
			CFRelease(options);
		}
		trusted
	}
}

#[link(name = "objc")]
unsafe extern "C" {
	fn objc_getClass(name: *const c_char) -> Id;
	fn sel_registerName(name: *const c_char) -> Sel;
	fn objc_msgSend();
	fn objc_autoreleasePoolPush() -> *mut c_void;
	fn objc_autoreleasePoolPop(pool: *mut c_void);
}

/// Posts one media key press or release, the way the keyboard driver does (an NSSystemDefined event).
unsafe fn post_media_key(key: i64, down: bool) {
	type OtherEvent = unsafe extern "C" fn(Id, Sel, u64, NSPoint, u64, f64, i64, Id, i16, i64, i64) -> Id;
	type GetCGEvent = unsafe extern "C" fn(Id, Sel) -> *mut c_void;
	unsafe {
		let pool = objc_autoreleasePoolPush();
		let class = objc_getClass(c"NSEvent".as_ptr());
		let make: OtherEvent = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
		let state: i64 = if down { 0xa } else { 0xb };
		let event = make(
			class,
			sel_registerName(c"otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:".as_ptr()),
			14, // NSEventTypeSystemDefined
			NSPoint { x: 0.0, y: 0.0 },
			(state << 8) as u64,
			0.0,
			0,
			std::ptr::null_mut(),
			8, // NX_SUBTYPE_AUX_CONTROL_BUTTONS
			(key << 16) | (state << 8),
			-1,
		);
		if !event.is_null() {
			let get: GetCGEvent = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
			let cg = get(event, sel_registerName(c"CGEvent".as_ptr()));
			if !cg.is_null() {
				CGEventPost(0, cg); // kCGHIDEventTap
			}
		}
		objc_autoreleasePoolPop(pool);
	}
}

static WARNED: AtomicBool = AtomicBool::new(false);

/// Whether macOS lets this process post keyboard events. Posting events is its own permission
/// (CGPreflightPostEventAccess), shown under Accessibility; AXIsProcessTrusted is checked too since older
/// macOS versions only know that one.
fn may_post_events() -> bool {
	unsafe { CGPreflightPostEventAccess() || AXIsProcessTrusted() }
}

/// Posts a media key press and release.
async fn media_key(key: i64) {
	log::info!("Sending media key {key}");
	let _ = tokio::task::spawn_blocking(move || unsafe {
		post_media_key(key, true);
		post_media_key(key, false);
	})
	.await;
}

/// Logs (once) and asks macOS for permission when the checks say events may not be posted.
fn check_permission() -> bool {
	if may_post_events() {
		return true;
	}
	if !WARNED.swap(true, Ordering::Relaxed) {
		let asked = unsafe { CGRequestPostEventAccess() } || ask_for_accessibility();
		log::warn!(
			"macOS says OpenDeck may not send keys (asked again: {asked}). Allow OpenDeck under System Settings > \
			 Privacy & Security > Accessibility; for a self-built OpenDeck, remove its entry and add the app again."
		);
	}
	false
}

// ---------- Players (AppleScript) ----------

fn apps(player: Player) -> &'static [&'static str] {
	match player {
		Player::Spotify => &["Spotify"],
		Player::Music => &["Music"],
		Player::Auto => &["Spotify", "Music"],
	}
}

/// The apps in `apps` that are open. Checked with pgrep, because an AppleScript naming an app that is not
/// installed pops up a "Where is ...?" dialog, and telling an app that is not open launches it.
async fn open_apps(apps: &[&'static str]) -> Vec<&'static str> {
	let mut open = vec![];
	for app in apps {
		if matches!(run("pgrep", &["-x", app]).await, Output::Ok(_)) {
			open.push(*app);
		}
	}
	open
}

/// AppleScript that runs `command` in the first of `apps` that is playing, else in the first one.
fn player_script(apps: &[&str], command: &str) -> String {
	let mut script = String::new();
	for app in apps {
		script.push_str(&format!(
			"tell application \"{app}\"\n\tif player state is playing then\n\t\t{command}\n\t\treturn \"{app}\"\n\tend if\nend tell\n"
		));
	}
	if let Some(app) = apps.first() {
		script.push_str(&format!("tell application \"{app}\" to {command}\nreturn \"{app}\""));
	}
	script
}

async fn tell_player(player: Player, command: &str) {
	let open = open_apps(apps(player)).await;
	if open.is_empty() {
		log::info!("Neither Spotify nor Music is open for: {command}");
		return;
	}
	if let Some(app) = osascript(&player_script(&open, command)).await {
		log::info!("{command} sent to {app}");
	}
}

async fn transport(player: Player, key: i64, command: &str) {
	if player != Player::Auto {
		return tell_player(player, command).await;
	}
	if !check_permission() {
		// The check can be wrong for self-built apps whose entry was granted for an older build. Prefer telling
		// Spotify or Music directly when one is open; otherwise send the key anyway, since it costs nothing.
		if !open_apps(apps(player)).await.is_empty() {
			return tell_player(player, command).await;
		}
		log::info!("No Spotify or Music open; sending media key {key} anyway");
	}
	media_key(key).await
}

pub async fn play_pause(player: Player) {
	transport(player, NX_KEYTYPE_PLAY, "playpause").await
}

pub async fn next(player: Player) {
	transport(player, NX_KEYTYPE_NEXT, "next track").await
}

pub async fn previous(player: Player) {
	transport(player, NX_KEYTYPE_PREVIOUS, "previous track").await
}

/// macOS has no system-wide seek, so this works with Spotify and Music.
pub async fn seek(player: Player, seconds: i64) {
	let command = format!("set player position to ((player position) + ({seconds}))");
	tell_player(player, &command).await
}

pub async fn list_players() {
	println!(
		"May post events: {} (CGPreflightPostEventAccess {}, AXIsProcessTrusted {})",
		may_post_events(),
		unsafe { CGPreflightPostEventAccess() },
		unsafe { AXIsProcessTrusted() }
	);
	println!("Open players: {:?}", open_apps(&["Spotify", "Music"]).await);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_report() {
		assert_eq!(parse_report("45,false"), Some(Level { percent: 45, muted: false }));
		assert_eq!(parse_report("0,true"), Some(Level { percent: 0, muted: true }));
		assert_eq!(parse_report("missing value,false"), None);
	}

	#[test]
	fn player_script_prefers_the_playing_app() {
		let s = player_script(&["Spotify", "Music"], "playpause");
		assert!(s.starts_with("tell application \"Spotify\"\n\tif player state is playing"));
		assert!(s.ends_with("tell application \"Spotify\" to playpause\nreturn \"Spotify\""));
	}
}
