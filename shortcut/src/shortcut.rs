//! Shortcut text such as `cmd+shift+4` or `ctrl+k ctrl+s`: parsing, the key table and key labels.
//!
//! Keys are physical positions, named after the US layout (like the browser's `KeyboardEvent.code`), so a
//! shortcut recorded on one keyboard presses the same key on the same keyboard every time.

/// One key: its name, other accepted spellings, macOS virtual key code, Linux evdev code and labels.
pub struct KeyDef {
	pub name: &'static str,
	pub aliases: &'static [&'static str],
	#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
	pub mac: u16,
	#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
	pub linux: u16,
	/// Label on macOS (symbols where macOS uses them in menus).
	pub mac_label: &'static str,
	/// Label on Linux.
	pub label: &'static str,
	/// macOS puts the Fn flag (and NumericPad for arrows) on these keys; synthetic events should match.
	pub mac_flags: u64,
}

pub const FLAG_SHIFT: u64 = 0x0002_0000;
pub const FLAG_CTRL: u64 = 0x0004_0000;
pub const FLAG_ALT: u64 = 0x0008_0000;
pub const FLAG_CMD: u64 = 0x0010_0000;
pub const FLAG_NUMPAD: u64 = 0x0020_0000;
pub const FLAG_FN: u64 = 0x0080_0000;
const ARROW: u64 = FLAG_FN | FLAG_NUMPAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier {
	Ctrl,
	Alt,
	Shift,
	Cmd,
}

impl Modifier {
	pub fn name(self) -> &'static str {
		match self {
			Modifier::Ctrl => "ctrl",
			Modifier::Alt => "alt",
			Modifier::Shift => "shift",
			Modifier::Cmd => "cmd",
		}
	}

	fn parse(s: &str) -> Option<Modifier> {
		Some(match s {
			"ctrl" | "control" | "ctl" | "⌃" => Modifier::Ctrl,
			"alt" | "opt" | "option" | "⌥" => Modifier::Alt,
			"shift" | "⇧" => Modifier::Shift,
			"cmd" | "command" | "meta" | "super" | "win" | "windows" | "⌘" => Modifier::Cmd,
			_ => return None,
		})
	}

	/// The left-hand key for this modifier.
	pub fn key(self) -> &'static KeyDef {
		find(self.name()).expect("modifier keys are in the table")
	}

	pub fn mac_flag(self) -> u64 {
		match self {
			Modifier::Ctrl => FLAG_CTRL,
			Modifier::Alt => FLAG_ALT,
			Modifier::Shift => FLAG_SHIFT,
			Modifier::Cmd => FLAG_CMD,
		}
	}

	fn label(self, mac: bool) -> &'static str {
		match (self, mac) {
			(Modifier::Ctrl, true) => "⌃",
			(Modifier::Alt, true) => "⌥",
			(Modifier::Shift, true) => "⇧",
			(Modifier::Cmd, true) => "⌘",
			(Modifier::Ctrl, false) => "Ctrl",
			(Modifier::Alt, false) => "Alt",
			(Modifier::Shift, false) => "Shift",
			(Modifier::Cmd, false) => "Super",
		}
	}
}

macro_rules! k {
	($name:literal, [$($alias:literal),*], $mac:expr, $linux:expr, $mac_label:literal, $label:literal) => {
		k!($name, [$($alias),*], $mac, $linux, $mac_label, $label, 0)
	};
	($name:literal, [$($alias:literal),*], $mac:expr, $linux:expr, $mac_label:literal, $label:literal, $flags:expr) => {
		KeyDef { name: $name, aliases: &[$($alias),*], mac: $mac, linux: $linux, mac_label: $mac_label, label: $label, mac_flags: $flags }
	};
}

pub static KEYS: &[KeyDef] = &[
	// Modifiers (left-hand keys).
	k!("ctrl", [], 0x3B, 29, "⌃", "Ctrl"),
	k!("alt", [], 0x3A, 56, "⌥", "Alt"),
	k!("shift", [], 0x38, 42, "⇧", "Shift"),
	k!("cmd", [], 0x37, 125, "⌘", "Super"),
	// Letters.
	k!("a", [], 0x00, 30, "A", "A"),
	k!("b", [], 0x0B, 48, "B", "B"),
	k!("c", [], 0x08, 46, "C", "C"),
	k!("d", [], 0x02, 32, "D", "D"),
	k!("e", [], 0x0E, 18, "E", "E"),
	k!("f", [], 0x03, 33, "F", "F"),
	k!("g", [], 0x05, 34, "G", "G"),
	k!("h", [], 0x04, 35, "H", "H"),
	k!("i", [], 0x22, 23, "I", "I"),
	k!("j", [], 0x26, 36, "J", "J"),
	k!("k", [], 0x28, 37, "K", "K"),
	k!("l", [], 0x25, 38, "L", "L"),
	k!("m", [], 0x2E, 50, "M", "M"),
	k!("n", [], 0x2D, 49, "N", "N"),
	k!("o", [], 0x1F, 24, "O", "O"),
	k!("p", [], 0x23, 25, "P", "P"),
	k!("q", [], 0x0C, 16, "Q", "Q"),
	k!("r", [], 0x0F, 19, "R", "R"),
	k!("s", [], 0x01, 31, "S", "S"),
	k!("t", [], 0x11, 20, "T", "T"),
	k!("u", [], 0x20, 22, "U", "U"),
	k!("v", [], 0x09, 47, "V", "V"),
	k!("w", [], 0x0D, 17, "W", "W"),
	k!("x", [], 0x07, 45, "X", "X"),
	k!("y", [], 0x10, 21, "Y", "Y"),
	k!("z", [], 0x06, 44, "Z", "Z"),
	// Digits.
	k!("1", [], 0x12, 2, "1", "1"),
	k!("2", [], 0x13, 3, "2", "2"),
	k!("3", [], 0x14, 4, "3", "3"),
	k!("4", [], 0x15, 5, "4", "4"),
	k!("5", [], 0x17, 6, "5", "5"),
	k!("6", [], 0x16, 7, "6", "6"),
	k!("7", [], 0x1A, 8, "7", "7"),
	k!("8", [], 0x1C, 9, "8", "8"),
	k!("9", [], 0x19, 10, "9", "9"),
	k!("0", [], 0x1D, 11, "0", "0"),
	// Punctuation.
	k!("minus", ["-"], 0x1B, 12, "-", "-"),
	k!("equal", ["=", "equals"], 0x18, 13, "=", "="),
	k!("leftbracket", ["[", "bracketleft"], 0x21, 26, "[", "["),
	k!("rightbracket", ["]", "bracketright"], 0x1E, 27, "]", "]"),
	k!("backslash", ["\\"], 0x2A, 43, "\\", "\\"),
	k!("semicolon", [";"], 0x29, 39, ";", ";"),
	k!("quote", ["'", "apostrophe"], 0x27, 40, "'", "'"),
	k!("comma", [","], 0x2B, 51, ",", ","),
	k!("period", [".", "dot"], 0x2F, 52, ".", "."),
	k!("slash", ["/"], 0x2C, 53, "/", "/"),
	k!("grave", ["`", "backquote", "backtick"], 0x32, 41, "`", "`"),
	k!("section", ["intlbackslash", "§"], 0x0A, 86, "§", "<>"),
	// Editing and navigation.
	k!("enter", ["return"], 0x24, 28, "↩", "Enter"),
	k!("tab", [], 0x30, 15, "⇥", "Tab"),
	k!("space", [], 0x31, 57, "Space", "Space"),
	k!("backspace", [], 0x33, 14, "⌫", "Bksp"),
	k!("delete", ["del", "forwarddelete"], 0x75, 111, "⌦", "Del", FLAG_FN),
	k!("escape", ["esc"], 0x35, 1, "Esc", "Esc"),
	k!("insert", ["ins", "help"], 0x72, 110, "Ins", "Ins", FLAG_FN),
	k!("home", [], 0x73, 102, "↖", "Home", FLAG_FN),
	k!("end", [], 0x77, 107, "↘", "End", FLAG_FN),
	k!("pageup", ["pgup"], 0x74, 104, "⇞", "PgUp", FLAG_FN),
	k!("pagedown", ["pgdn"], 0x79, 109, "⇟", "PgDn", FLAG_FN),
	k!("left", ["arrowleft"], 0x7B, 105, "←", "←", ARROW),
	k!("right", ["arrowright"], 0x7C, 106, "→", "→", ARROW),
	k!("up", ["arrowup"], 0x7E, 103, "↑", "↑", ARROW),
	k!("down", ["arrowdown"], 0x7D, 108, "↓", "↓", ARROW),
	k!("capslock", ["caps"], 0x39, 58, "⇪", "Caps"),
	// Function keys. macOS has no F21-F24.
	k!("f1", [], 0x7A, 59, "F1", "F1", FLAG_FN),
	k!("f2", [], 0x78, 60, "F2", "F2", FLAG_FN),
	k!("f3", [], 0x63, 61, "F3", "F3", FLAG_FN),
	k!("f4", [], 0x76, 62, "F4", "F4", FLAG_FN),
	k!("f5", [], 0x60, 63, "F5", "F5", FLAG_FN),
	k!("f6", [], 0x61, 64, "F6", "F6", FLAG_FN),
	k!("f7", [], 0x62, 65, "F7", "F7", FLAG_FN),
	k!("f8", [], 0x64, 66, "F8", "F8", FLAG_FN),
	k!("f9", [], 0x65, 67, "F9", "F9", FLAG_FN),
	k!("f10", [], 0x6D, 68, "F10", "F10", FLAG_FN),
	k!("f11", [], 0x67, 87, "F11", "F11", FLAG_FN),
	k!("f12", [], 0x6F, 88, "F12", "F12", FLAG_FN),
	k!("f13", ["printscreen", "print"], 0x69, 183, "F13", "F13", FLAG_FN),
	k!("f14", ["scrolllock"], 0x6B, 184, "F14", "F14", FLAG_FN),
	k!("f15", ["pause"], 0x71, 185, "F15", "F15", FLAG_FN),
	k!("f16", [], 0x6A, 186, "F16", "F16", FLAG_FN),
	k!("f17", [], 0x40, 187, "F17", "F17", FLAG_FN),
	k!("f18", [], 0x4F, 188, "F18", "F18", FLAG_FN),
	k!("f19", [], 0x50, 189, "F19", "F19", FLAG_FN),
	k!("f20", [], 0x5A, 190, "F20", "F20", FLAG_FN),
	// Number pad.
	k!("kp0", ["numpad0"], 0x52, 82, "0", "Num0", FLAG_NUMPAD),
	k!("kp1", ["numpad1"], 0x53, 79, "1", "Num1", FLAG_NUMPAD),
	k!("kp2", ["numpad2"], 0x54, 80, "2", "Num2", FLAG_NUMPAD),
	k!("kp3", ["numpad3"], 0x55, 81, "3", "Num3", FLAG_NUMPAD),
	k!("kp4", ["numpad4"], 0x56, 75, "4", "Num4", FLAG_NUMPAD),
	k!("kp5", ["numpad5"], 0x57, 76, "5", "Num5", FLAG_NUMPAD),
	k!("kp6", ["numpad6"], 0x58, 77, "6", "Num6", FLAG_NUMPAD),
	k!("kp7", ["numpad7"], 0x59, 71, "7", "Num7", FLAG_NUMPAD),
	k!("kp8", ["numpad8"], 0x5B, 72, "8", "Num8", FLAG_NUMPAD),
	k!("kp9", ["numpad9"], 0x5C, 73, "9", "Num9", FLAG_NUMPAD),
	k!("kpdecimal", ["numpaddecimal", "kpdot"], 0x41, 83, ".", "Num.", FLAG_NUMPAD),
	k!("kpplus", ["numpadadd", "kpadd"], 0x45, 78, "+", "Num+", FLAG_NUMPAD),
	k!("kpminus", ["numpadsubtract", "kpsubtract"], 0x4E, 74, "-", "Num-", FLAG_NUMPAD),
	k!("kpmultiply", ["numpadmultiply", "kpasterisk"], 0x43, 55, "*", "Num*", FLAG_NUMPAD),
	k!("kpdivide", ["numpaddivide", "kpslash"], 0x4B, 98, "/", "Num/", FLAG_NUMPAD),
	k!("kpenter", ["numpadenter"], 0x4C, 96, "⌤", "NumEnter", FLAG_NUMPAD),
	k!("kpequal", ["numpadequal"], 0x51, 117, "=", "Num=", FLAG_NUMPAD),
];

pub fn find(name: &str) -> Option<&'static KeyDef> {
	KEYS.iter().find(|k| k.name == name || k.aliases.contains(&name))
}

/// One key combination: modifiers held while one key is pressed. `key` is None for a modifier-only combo.
#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
	pub mods: Vec<Modifier>,
	pub key: Option<&'static KeyDef>,
}

impl Chord {
	pub fn mac_flags(&self) -> u64 {
		self.mods.iter().fold(0, |f, m| f | m.mac_flag())
	}

	pub fn canonical(&self) -> String {
		let mut parts: Vec<&str> = self.mods.iter().map(|m| m.name()).collect();
		if let Some(k) = self.key {
			parts.push(k.name);
		}
		parts.join("+")
	}

	pub fn label(&self, mac: bool) -> String {
		let mods: Vec<&str> = self.mods.iter().map(|m| m.label(mac)).collect();
		let key = self.key.map(|k| if mac { k.mac_label } else { k.label });
		if mac {
			format!("{}{}", mods.concat(), key.unwrap_or_default())
		} else {
			mods.into_iter().chain(key).collect::<Vec<_>>().join("+")
		}
	}
}

impl PartialEq for KeyDef {
	fn eq(&self, other: &Self) -> bool {
		self.name == other.name
	}
}

impl std::fmt::Debug for KeyDef {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.name)
	}
}

/// A shortcut: one chord, or several pressed one after another (`ctrl+k ctrl+s`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shortcut(pub Vec<Chord>);

pub const MAX_CHORDS: usize = 8;

impl Shortcut {
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	pub fn canonical(&self) -> String {
		self.0.iter().map(Chord::canonical).collect::<Vec<_>>().join(" ")
	}

	pub fn label(&self, mac: bool) -> String {
		self.0.iter().map(|c| c.label(mac)).collect::<Vec<_>>().join(" ")
	}
}

fn parse_chord(text: &str) -> Result<Chord, String> {
	// "cmd++" or "+" alone means the "=" key with shift, which is what "+" is on a US keyboard.
	let (text, plus) = match text.strip_suffix("++").or_else(|| (text == "+").then_some("")) {
		Some(rest) => (rest, true),
		None => (text, false),
	};
	let mut mods = vec![];
	let mut key = None;
	for part in text.split('+').filter(|p| !p.is_empty()) {
		let lower = part.to_lowercase();
		if let Some(m) = Modifier::parse(&lower) {
			if !mods.contains(&m) {
				mods.push(m);
			}
			continue;
		}
		let Some(def) = find(&lower) else { return Err(format!("Unknown key \"{part}\"")) };
		if let Some(prev) = key.replace(def) {
			return Err(format!("\"{}\" and \"{}\" are both keys; a combo has one key plus modifiers", prev.name, def.name));
		}
	}
	if plus {
		if let Some(prev) = key {
			return Err(format!("\"{}\" and \"+\" are both keys", prev.name));
		}
		if !mods.contains(&Modifier::Shift) {
			mods.push(Modifier::Shift);
		}
		key = find("equal");
	}
	if mods.is_empty() && key.is_none() {
		return Err("Empty combo".into());
	}
	mods.sort();
	Ok(Chord { mods, key })
}

/// Parses shortcut text. Combos are separated by spaces; keys within a combo by `+`.
pub fn parse(text: &str) -> Result<Shortcut, String> {
	let chords = text.split_whitespace().map(parse_chord).collect::<Result<Vec<_>, _>>()?;
	if chords.len() > MAX_CHORDS {
		return Err(format!("At most {MAX_CHORDS} combos in a row"));
	}
	Ok(Shortcut(chords))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_and_normalises() {
		let s = parse("Shift+Cmd+4").unwrap();
		assert_eq!(s.canonical(), "shift+cmd+4");
		assert_eq!(s.label(true), "⇧⌘4");
		assert_eq!(s.label(false), "Shift+Super+4");
		assert_eq!(parse("command+option+esc").unwrap().canonical(), "alt+cmd+escape");
		assert_eq!(parse("ctrl+alt+t").unwrap().label(true), "⌃⌥T");
		assert_eq!(parse("⌘⇧").map_err(|e| e.contains("Unknown")), Err(true));
	}

	#[test]
	fn mac_order_is_ctrl_alt_shift_cmd() {
		assert_eq!(parse("cmd+shift+alt+ctrl+z").unwrap().label(true), "⌃⌥⇧⌘Z");
	}

	#[test]
	fn sequences_and_modifier_only() {
		let s = parse("ctrl+k  ctrl+s").unwrap();
		assert_eq!(s.0.len(), 2);
		assert_eq!(s.canonical(), "ctrl+k ctrl+s");
		let m = parse("ctrl").unwrap();
		assert_eq!(m.0[0].key, None);
		assert_eq!(m.0[0].mods, vec![Modifier::Ctrl]);
		assert!(parse("").unwrap().is_empty());
	}

	#[test]
	fn plus_key() {
		assert_eq!(parse("cmd++").unwrap().canonical(), "shift+cmd+equal");
		assert_eq!(parse("+").unwrap().canonical(), "shift+equal");
		assert_eq!(parse("cmd+=").unwrap().canonical(), "cmd+equal");
	}

	#[test]
	fn errors() {
		assert!(parse("cmd+foo").unwrap_err().contains("foo"));
		assert!(parse("a+b").is_err());
	}

	#[test]
	fn table_has_unique_names_and_codes() {
		let mut names = std::collections::HashSet::new();
		let mut mac = std::collections::HashSet::new();
		let mut linux = std::collections::HashSet::new();
		for k in KEYS {
			assert!(names.insert(k.name), "duplicate {}", k.name);
			for a in k.aliases {
				assert!(names.insert(a), "duplicate alias {a}");
			}
			assert!(mac.insert(k.mac), "duplicate mac code for {}", k.name);
			assert!(linux.insert(k.linux), "duplicate linux code for {}", k.name);
			assert!(k.linux < 256);
		}
	}
}
