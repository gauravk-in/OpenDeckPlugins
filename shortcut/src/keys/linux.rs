//! Linux: a virtual keyboard through /dev/uinput. Works the same under X11, Wayland and on the console,
//! because the events come from a (virtual) input device rather than from the display server.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::shortcut::KeyDef;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0;
const BUS_VIRTUAL: u16 = 0x06;

// _IOW('U', 100, int), _IOW('U', 101, int), _IO('U', 1), _IOW('U', 3, struct uinput_setup)
const UI_SET_EVBIT: u64 = 0x4004_5564;
const UI_SET_KEYBIT: u64 = 0x4004_5565;
const UI_DEV_CREATE: u64 = 0x5501;
const UI_DEV_SETUP: u64 = 0x405c_5503;

#[repr(C)]
struct InputId {
	bustype: u16,
	vendor: u16,
	product: u16,
	version: u16,
}

#[repr(C)]
struct UinputSetup {
	id: InputId,
	name: [u8; 80],
	ff_effects_max: u32,
}

#[repr(C)]
struct InputEvent {
	time: libc::timeval,
	kind: u16,
	code: u16,
	value: i32,
}

struct Device {
	file: File,
	created: Instant,
}

static DEVICE: Mutex<Option<Device>> = Mutex::new(None);

pub const PATH: &str = "/dev/uinput";

fn ioctl(file: &File, request: u64, arg: libc::c_ulong) -> std::io::Result<()> {
	// SAFETY: the requests above take an int or a pointer to a live uinput_setup.
	let r = unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg) };
	if r < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

fn create() -> Result<Device, String> {
	let path = std::env::var("OPENDECK_SHORTCUT_UINPUT").unwrap_or_else(|_| PATH.into());
	let file = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(&path).map_err(|e| match e.kind() {
		std::io::ErrorKind::PermissionDenied => format!(
			"No permission to use {path}. Install the udev rule from the plugin's README (70-opendeck-shortcut.rules), \
			 then log out and in again."
		),
		std::io::ErrorKind::NotFound => format!("{path} is missing. Load the module with: sudo modprobe uinput"),
		_ => format!("Cannot open {path}: {e}"),
	})?;
	let setup_err = |e: std::io::Error| format!("Cannot set up the virtual keyboard: {e}");
	ioctl(&file, UI_SET_EVBIT, EV_KEY as _).map_err(setup_err)?;
	for code in 1..256u16 {
		ioctl(&file, UI_SET_KEYBIT, code as _).map_err(setup_err)?;
	}
	let mut setup = UinputSetup {
		id: InputId { bustype: BUS_VIRTUAL, vendor: 0x4f44, product: 0x5343, version: 1 },
		name: [0; 80],
		ff_effects_max: 0,
	};
	let name = b"OpenDeck Keyboard Shortcut";
	setup.name[..name.len()].copy_from_slice(name);
	ioctl(&file, UI_DEV_SETUP, &raw const setup as _).map_err(setup_err)?;
	ioctl(&file, UI_DEV_CREATE, 0).map_err(setup_err)?;
	log::info!("Virtual keyboard created on {path}");
	Ok(Device { file, created: Instant::now() })
}

/// Creates the virtual keyboard if needed. Creating it early gives the desktop time to pick it up.
pub fn ready() -> Result<(), String> {
	let mut dev = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
	if dev.is_none() {
		*dev = Some(create()?);
	}
	Ok(())
}

pub fn backend() -> &'static str {
	"uinput"
}

fn write_event(file: &mut File, kind: u16, code: u16, value: i32) -> std::io::Result<()> {
	let ev = InputEvent { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, kind, code, value };
	// SAFETY: InputEvent is plain old data with the kernel's layout.
	let bytes = unsafe { std::slice::from_raw_parts(&raw const ev as *const u8, std::mem::size_of::<InputEvent>()) };
	file.write_all(bytes)
}

pub fn send(key: &KeyDef, down: bool, _mac_flags: u64) -> Result<(), String> {
	let mut guard = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
	if guard.is_none() {
		*guard = Some(create()?);
	}
	let dev = guard.as_mut().expect("created above");
	// A brand-new input device is ignored until the desktop has opened it.
	let age = dev.created.elapsed();
	if age < Duration::from_millis(500) {
		std::thread::sleep(Duration::from_millis(500) - age);
	}
	let result = write_event(&mut dev.file, EV_KEY, key.linux, down as i32)
		.and_then(|_| write_event(&mut dev.file, EV_SYN, SYN_REPORT, 0));
	if let Err(e) = result {
		// Drop the device so the next press tries again from scratch.
		*guard = None;
		return Err(format!("Sending a key failed: {e}"));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn structs_match_the_kernel_layout() {
		// UI_DEV_SETUP encodes sizeof(struct uinput_setup) = 92; struct input_event is 24 bytes on 64-bit.
		assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
		assert_eq!((UI_DEV_SETUP >> 16) & 0x3fff, 92);
		assert_eq!(std::mem::size_of::<InputEvent>(), 24);
	}
}
