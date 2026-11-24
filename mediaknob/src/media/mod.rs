//! Talking to the system: output volume, and the media player.
//!
//! macOS: volume and mute through AppleScript (`osascript`); play/pause and tracks through the
//! keyboard's media keys, which go to whatever app is playing (Music, Spotify, a browser, ...);
//! seeking and a chosen player through Spotify's or Music's AppleScript.
//!
//! Linux: volume and mute through `wpctl` (PipeWire), `pactl` (PulseAudio) or `amixer` (ALSA),
//! whichever works; the player through MPRIS on the session D-Bus.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as platform;

pub(crate) mod run;

/// The output volume, 0 to 100, and whether it is muted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
	pub percent: u8,
	pub muted: bool,
}

/// Which player gets play/pause, track and seek commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Player {
	/// Whatever is playing now (or was played last).
	Auto,
	Spotify,
	/// Apple Music on macOS.
	Music,
}

impl Player {
	pub fn from_setting(s: &str) -> Self {
		match s {
			"spotify" => Player::Spotify,
			"music" => Player::Music,
			_ => Player::Auto,
		}
	}
}

pub async fn level() -> Option<Level> {
	platform::level().await
}

/// Changes the volume by `delta` percent (raising it also unmutes), and returns the new level.
pub async fn change_volume(delta: i32) -> Option<Level> {
	platform::change_volume(delta).await
}

pub async fn toggle_mute() -> Option<Level> {
	platform::toggle_mute().await
}

pub async fn play_pause(player: Player) {
	platform::play_pause(player).await
}

pub async fn next(player: Player) {
	platform::next(player).await
}

pub async fn previous(player: Player) {
	platform::previous(player).await
}

/// Moves the playing track `seconds` forward (or back when negative).
pub async fn seek(player: Player, seconds: i64) {
	platform::seek(player, seconds).await
}

/// Prints the players that can be controlled (debug helper).
pub async fn list_players() {
	platform::list_players().await
}
