//! macOS: keyboard events posted with CoreGraphics. macOS only delivers them when the app that started the
//! plugin (OpenDeck) is allowed under System Settings > Privacy & Security > Accessibility.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::shortcut::KeyDef;

type CFTypeRef = *const c_void;

const HID_SYSTEM_STATE: i32 = 1;
const HID_EVENT_TAP: u32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
	fn CGEventSourceCreate(state: i32) -> CFTypeRef;
	fn CGEventCreateKeyboardEvent(source: CFTypeRef, keycode: u16, down: bool) -> CFTypeRef;
	fn CGEventSetFlags(event: CFTypeRef, flags: u64);
	fn CGEventPost(tap: u32, event: CFTypeRef);
	fn CGPreflightPostEventAccess() -> bool;
	fn CGRequestPostEventAccess() -> bool;
	fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	fn CFRelease(cf: CFTypeRef);
}

pub const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

const NOT_ALLOWED: &str = "macOS does not let OpenDeck send keys yet. Allow OpenDeck under System Settings > \
	Privacy & Security > Accessibility, then restart OpenDeck. If it is already listed, switching it off and on is \
	not enough: remove it with the minus button and add it again, because each OpenDeck build needs its own entry. \
	A self-built OpenDeck must also be signed first, or macOS ignores the entry: \
	codesign --force --deep -s - \"/Applications/OpenDeck Pages.app\"";

fn allowed() -> bool {
	unsafe { CGPreflightPostEventAccess() || AXIsProcessTrusted() }
}

static ASKED: AtomicBool = AtomicBool::new(false);

/// Shows macOS's own prompt, which also adds the running OpenDeck to the Accessibility list. Without it, a
/// differently built OpenDeck (another bundle id or signature) never shows up there. Once per run.
pub fn ask_for_access() {
	if !ASKED.swap(true, Ordering::Relaxed) && !allowed() {
		unsafe { CGRequestPostEventAccess() };
	}
}

pub fn ready() -> Result<(), String> {
	if allowed() {
		return Ok(());
	}
	ask_for_access();
	Err(NOT_ALLOWED.into())
}

pub fn backend() -> &'static str {
	"CoreGraphics"
}

pub fn send(key: &KeyDef, down: bool, flags: u64) -> Result<(), String> {
	unsafe {
		let source = CGEventSourceCreate(HID_SYSTEM_STATE);
		let event = CGEventCreateKeyboardEvent(source, key.mac, down);
		if event.is_null() {
			if !source.is_null() {
				CFRelease(source);
			}
			return Err("macOS could not create a key event".into());
		}
		CGEventSetFlags(event, flags);
		CGEventPost(HID_EVENT_TAP, event);
		CFRelease(event);
		if !source.is_null() {
			CFRelease(source);
		}
	}
	Ok(())
}
