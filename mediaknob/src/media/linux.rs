use std::sync::Mutex;

use tokio::sync::OnceCell;
use zbus::Connection;

use super::run::{Output, run};
use super::{Level, Player};

// ---------- Volume ----------

/// The tool that sets the volume on this machine.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mixer {
	/// PipeWire (Raspberry Pi OS Bookworm, Fedora, Ubuntu 22.10+).
	Wpctl,
	/// PulseAudio, or PipeWire with pipewire-pulse.
	Pactl,
	/// Plain ALSA, with this simple control.
	Amixer(&'static str),
}

const ALL_MIXERS: [Mixer; 4] = [Mixer::Wpctl, Mixer::Pactl, Mixer::Amixer("Master"), Mixer::Amixer("PCM")];
static MIXER: Mutex<Option<Mixer>> = Mutex::new(None);

const WP_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const PA_SINK: &str = "@DEFAULT_SINK@";

/// Parses `wpctl get-volume`: "Volume: 0.45" or "Volume: 0.45 [MUTED]".
fn parse_wpctl(out: &str) -> Option<Level> {
	let value: f64 = out.split_whitespace().nth(1)?.parse().ok()?;
	Some(Level { percent: (value * 100.0).round().clamp(0.0, 255.0) as u8, muted: out.contains("[MUTED]") })
}

/// First "NN%" in pactl's volume line ("Volume: front-left: 29491 /  45% / -20.81 dB, ...").
fn parse_percent(out: &str) -> Option<u8> {
	let end = out.find('%')?;
	let start = out[..end].rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
	out[start..end].parse().ok()
}

/// Parses `amixer -M get <control>`: "... [45%] ... [on]".
fn parse_amixer(out: &str) -> Option<Level> {
	let percent = out.match_indices('[').find_map(|(i, _)| {
		let rest = &out[i + 1..];
		let close = rest.find(']')?;
		rest[..close].strip_suffix('%')?.parse().ok()
	})?;
	Some(Level { percent, muted: out.contains("[off]") })
}

async fn mixer_level(mixer: Mixer) -> Option<Level> {
	match mixer {
		Mixer::Wpctl => match run("wpctl", &["get-volume", WP_SINK]).await {
			Output::Ok(out) => parse_wpctl(&out),
			_ => None,
		},
		Mixer::Pactl => {
			let Output::Ok(volume) = run("pactl", &["get-sink-volume", PA_SINK]).await else { return None };
			let muted = matches!(run("pactl", &["get-sink-mute", PA_SINK]).await, Output::Ok(m) if m.contains("yes"));
			Some(Level { percent: parse_percent(&volume)?, muted })
		}
		Mixer::Amixer(control) => match run("amixer", &["-M", "get", control]).await {
			Output::Ok(out) => parse_amixer(&out),
			_ => None,
		},
	}
}

/// The mixer found last time, or the first one that answers.
async fn mixer() -> Option<Mixer> {
	if let Some(m) = *MIXER.lock().unwrap() {
		return Some(m);
	}
	for m in ALL_MIXERS {
		if mixer_level(m).await.is_some() {
			log::info!("Using {m:?} for volume");
			*MIXER.lock().unwrap() = Some(m);
			return Some(m);
		}
	}
	log::warn!("No working volume control found (tried wpctl, pactl, amixer)");
	None
}

async fn checked(what: &str, out: Output) -> bool {
	match out {
		Output::Ok(_) => true,
		Output::Missing => {
			log::warn!("{what}: program not installed");
			*MIXER.lock().unwrap() = None;
			false
		}
		Output::Failed(e) => {
			log::warn!("{what} failed: {e}");
			*MIXER.lock().unwrap() = None;
			false
		}
	}
}

pub async fn level() -> Option<Level> {
	let m = mixer().await?;
	let level = mixer_level(m).await;
	if level.is_none() {
		*MIXER.lock().unwrap() = None;
	}
	level
}

pub async fn change_volume(delta: i32) -> Option<Level> {
	let m = mixer().await?;
	let amount = format!("{}%{}", delta.unsigned_abs(), if delta > 0 { '+' } else { '-' });
	let ok = match m {
		Mixer::Wpctl => checked("wpctl", run("wpctl", &["set-volume", "-l", "1.0", WP_SINK, &amount]).await).await,
		Mixer::Pactl => {
			// pactl happily goes past 100%; set an absolute, clamped value instead.
			let current = mixer_level(m).await.map_or(0, |l| l.percent as i32);
			let target = format!("{}%", (current + delta).clamp(0, 100));
			checked("pactl", run("pactl", &["set-sink-volume", PA_SINK, &target]).await).await
		}
		Mixer::Amixer(control) => checked("amixer", run("amixer", &["-q", "-M", "set", control, &amount]).await).await,
	};
	if ok && delta > 0 {
		// Turning the volume up should be heard: unmute, like the keyboard's volume keys do.
		let _ = match m {
			Mixer::Wpctl => run("wpctl", &["set-mute", WP_SINK, "0"]).await,
			Mixer::Pactl => run("pactl", &["set-sink-mute", PA_SINK, "0"]).await,
			Mixer::Amixer(control) => run("amixer", &["-q", "set", control, "unmute"]).await,
		};
	}
	mixer_level(m).await
}

pub async fn toggle_mute() -> Option<Level> {
	let m = mixer().await?;
	let out = match m {
		Mixer::Wpctl => run("wpctl", &["set-mute", WP_SINK, "toggle"]).await,
		Mixer::Pactl => run("pactl", &["set-sink-mute", PA_SINK, "toggle"]).await,
		Mixer::Amixer(control) => run("amixer", &["-q", "set", control, "toggle"]).await,
	};
	if let Output::Failed(e) = &out {
		log::warn!("Mute with {m:?} failed: {e}");
	}
	mixer_level(m).await
}

// ---------- Player (MPRIS) ----------

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

static BUS: OnceCell<Option<Connection>> = OnceCell::const_new();
/// The player that got the last command, preferred when nothing is playing.
static LAST_PLAYER: Mutex<Option<String>> = Mutex::new(None);

async fn bus() -> Option<&'static Connection> {
	BUS.get_or_init(|| async {
		match Connection::session().await {
			Ok(c) => Some(c),
			Err(e) => {
				log::warn!("No session D-Bus, so no media player control: {e}");
				None
			}
		}
	})
	.await
	.as_ref()
}

async fn player_proxy(conn: &Connection, name: &str) -> Option<zbus::Proxy<'static>> {
	zbus::Proxy::new(conn, name.to_owned(), MPRIS_PATH, MPRIS_PLAYER).await.ok()
}

async fn players(conn: &Connection) -> Vec<(String, String)> {
	let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else { return vec![] };
	let Ok(names) = dbus.list_names().await else { return vec![] };
	let mut out = vec![];
	for name in names.iter().map(|n| n.as_str()).filter(|n| n.starts_with(MPRIS_PREFIX)) {
		let status = match player_proxy(conn, name).await {
			Some(p) => p.get_property::<String>("PlaybackStatus").await.unwrap_or_default(),
			None => String::new(),
		};
		out.push((name.to_owned(), status));
	}
	out
}

/// Picks the player to control: a playing one, else the one used last, else a paused one, else any.
fn choose(players: &[(String, String)], wanted: Player, last: Option<&str>) -> Option<String> {
	let candidates: Vec<&(String, String)> = match wanted {
		Player::Spotify => players.iter().filter(|(n, _)| n.to_lowercase().contains("spotify")).collect(),
		_ => players.iter().collect(),
	};
	let by_status = |s: &str| candidates.iter().find(|(_, st)| st == s).map(|(n, _)| n.clone());
	by_status("Playing")
		.or_else(|| last.and_then(|l| candidates.iter().find(|(n, _)| n == l).map(|(n, _)| n.clone())))
		.or_else(|| by_status("Paused"))
		.or_else(|| candidates.first().map(|(n, _)| n.clone()))
}

async fn with_player(wanted: Player, method: &str, body: impl serde::Serialize + zbus::zvariant::DynamicType) {
	let Some(conn) = bus().await else { return };
	let all = players(conn).await;
	let last = LAST_PLAYER.lock().unwrap().clone();
	let Some(name) = choose(&all, wanted, last.as_deref()) else {
		log::info!("{method}: no media player found");
		return;
	};
	let Some(proxy) = player_proxy(conn, &name).await else { return };
	match proxy.call_method(method, &body).await {
		Ok(_) => log::debug!("{method} sent to {name}"),
		Err(e) => log::warn!("{method} on {name} failed: {e}"),
	}
	*LAST_PLAYER.lock().unwrap() = Some(name);
}

pub async fn play_pause(player: Player) {
	with_player(player, "PlayPause", ()).await
}

pub async fn next(player: Player) {
	with_player(player, "Next", ()).await
}

pub async fn previous(player: Player) {
	with_player(player, "Previous", ()).await
}

pub async fn seek(player: Player, seconds: i64) {
	with_player(player, "Seek", (seconds * 1_000_000,)).await
}

pub async fn list_players() {
	let Some(conn) = bus().await else { return };
	for (name, status) in players(conn).await {
		println!("{name}\t{status}");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_mixer_output() {
		assert_eq!(parse_wpctl("Volume: 0.45\n"), Some(Level { percent: 45, muted: false }));
		assert_eq!(parse_wpctl("Volume: 1.00 [MUTED]\n"), Some(Level { percent: 100, muted: true }));
		assert_eq!(
			parse_percent("Volume: front-left: 29491 /  45% / -20.81 dB,   front-right: 29491 /  45% / -20.81 dB"),
			Some(45)
		);
		let amixer = "Simple mixer control 'Master',0\n  Front Left: Playback 26304 [40%] [-20.00dB] [off]\n";
		assert_eq!(parse_amixer(amixer), Some(Level { percent: 40, muted: true }));
		let pcm = "Simple mixer control 'PCM',0\n  Mono: Playback -2000 [77%] [-20.00dB]\n";
		assert_eq!(parse_amixer(pcm), Some(Level { percent: 77, muted: false }));
	}

	#[test]
	fn chooses_player() {
		let p = |n: &str, s: &str| (format!("{MPRIS_PREFIX}{n}"), s.to_owned());
		let all = vec![p("vlc", "Paused"), p("spotify", "Stopped"), p("firefox.instance1", "Playing")];
		assert_eq!(choose(&all, Player::Auto, None).unwrap(), all[2].0);
		assert_eq!(choose(&all, Player::Spotify, None).unwrap(), all[1].0);
		let idle = vec![p("vlc", "Stopped"), p("spotify", "Paused")];
		assert_eq!(choose(&idle, Player::Auto, None).unwrap(), idle[1].0);
		assert_eq!(choose(&idle, Player::Auto, Some(&idle[0].0)).unwrap(), idle[0].0);
		assert_eq!(choose(&[], Player::Auto, None), None);
	}
}
