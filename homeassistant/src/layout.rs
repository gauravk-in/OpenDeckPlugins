//! The shape of a deck: how many keys, which have screens, and how many knobs. Decides which keys show content
//! and which are used for Back / Home / More.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
	Back,
	Home,
	Next,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
	pub name: String,
	pub keys: usize,
	pub columns: usize,
	/// Per key: whether it has a screen.
	pub display: Vec<bool>,
	pub encoders: usize,
	/// Per key: whether this plugin's action is on it. Keys with other actions are left alone.
	pub owned: Vec<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slots {
	/// Keys that show items, in reading order.
	pub content: Vec<usize>,
	pub nav: Vec<(usize, Nav)>,
}

/// Decks in the Mirabox N3 family (Ajazz AKP03, Streonor S6, Soomfon SE, ...): 6 keys with screens, 3 plain
/// buttons below them, and 3 knobs. The opendeck-akp03 plugin registers them as 3x3 with keys 6-8 being the
/// plain buttons.
const N3_FAMILY: [&str; 9] = ["streonor s6", "akp03", "mirabox n3", "n3", "soomfon", "msd-two", "treaslin", "ss-551", "skyrider"];

impl Layout {
	/// A plain grid where every key has a screen.
	pub fn grid(rows: usize, columns: usize, encoders: usize) -> Self {
		let keys = rows * columns;
		Self { name: format!("{rows}x{columns}"), keys, columns, display: vec![true; keys], encoders, owned: vec![true; keys] }
	}

	pub fn streonor_s6() -> Self {
		let mut l = Self::grid(3, 3, 3);
		l.name = "Streonor S6".into();
		for d in &mut l.display[6..] {
			*d = false;
		}
		l
	}

	/// Works out the layout from what OpenDeck tells plugins about a device. `encoders` comes from the device's
	/// existing pages when known (OpenDeck does not include it in deviceDidConnect).
	pub fn for_device(name: &str, rows: u8, columns: u8, r#type: u8, encoders: Option<usize>) -> Self {
		let lower = name.to_lowercase();
		let (rows, columns) = (rows as usize, columns as usize);
		if rows == 3 && columns == 3 && N3_FAMILY.iter().any(|n| lower.contains(n)) {
			let mut l = Self::streonor_s6();
			l.name = name.to_owned();
			if let Some(e) = encoders {
				l.encoders = e;
			}
			return l;
		}
		// Elgato types: 7 = Stream Deck +, 13 = Stream Deck + XL.
		let default_encoders = match r#type {
			7 => 4,
			13 => 6,
			_ => 0,
		};
		let mut l = Self::grid(rows.max(1), columns.max(1), encoders.unwrap_or(default_encoders));
		l.name = name.to_owned();
		l
	}

	/// Assigns keys for a screen with `count` items. `can_go_back` is false on the room list.
	pub fn slots(&self, count: usize, can_go_back: bool) -> Slots {
		let mine = |k: &usize| self.owned.get(*k).copied().unwrap_or(false);
		let mut content: Vec<usize> = (0..self.keys).filter(mine).filter(|k| self.display[*k]).collect();
		let plain: Vec<usize> = (0..self.keys).filter(mine).filter(|k| !self.display[*k]).collect();
		let mut nav: Vec<(usize, Nav)> = match plain.len() {
			0 => vec![],
			1 => vec![(plain[0], Nav::Back)],
			2 => vec![(plain[0], Nav::Back), (plain[1], Nav::Next)],
			_ => vec![(plain[0], Nav::Back), (plain[1], Nav::Home), (plain[2], Nav::Next)],
		};
		if can_go_back && !nav.iter().any(|(_, n)| *n == Nav::Back) && content.len() > 1 {
			nav.push((content.pop().unwrap(), Nav::Back));
		}
		if count > content.len() && !nav.iter().any(|(_, n)| *n == Nav::Next) && content.len() > 1 {
			nav.push((content.pop().unwrap(), Nav::Next));
		}
		Slots { content, nav }
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn s6_is_detected_from_its_name() {
		let l = Layout::for_device("Streonor S6", 3, 3, 0, None);
		assert_eq!(l.encoders, 3);
		assert_eq!(l.display, [true, true, true, true, true, true, false, false, false]);
		let s = l.slots(20, true);
		assert_eq!(s.content, [0, 1, 2, 3, 4, 5]);
		assert_eq!(s.nav, [(6, Nav::Back), (7, Nav::Home), (8, Nav::Next)]);
	}

	#[test]
	fn elgato_plus_has_four_knobs() {
		let l = Layout::for_device("Stream Deck +", 2, 4, 7, None);
		assert_eq!((l.keys, l.encoders), (8, 4));
		assert_eq!(l.slots(3, false).content.len(), 8);
		assert_eq!(l.slots(3, true).nav, [(7, Nav::Back)]);
	}

	#[test]
	fn only_keys_with_this_action_are_used() {
		let mut l = Layout::grid(2, 3, 0);
		l.owned = vec![true, true, false, true, false, false];
		let s = l.slots(10, true);
		assert_eq!(s.content, [0]);
		assert_eq!(s.nav, [(3, Nav::Back), (1, Nav::Next)]);
	}

	#[test]
	fn encoders_from_existing_pages_win() {
		assert_eq!(Layout::for_device("Some deck", 2, 3, 0, Some(2)).encoders, 2);
	}
}
