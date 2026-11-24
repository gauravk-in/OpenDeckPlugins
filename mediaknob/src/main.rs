mod media;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use openaction::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use media::{Level, Player};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
	/// What turning the knob does: "volume", "seek", "track" or "none".
	rotate: String,
	/// What pressing the knob does: "playPause", "mute", "next", "previous" or "none".
	press: String,
	/// What turning the knob while holding it down does (same choices as `rotate`).
	press_rotate: String,
	/// Volume change per tick, in percent.
	volume_step: u8,
	/// Seek distance per tick, in seconds.
	seek_step: u32,
	/// Swap the turning direction.
	reverse: bool,
	/// Which player gets play/pause, track and seek commands: "auto", "spotify" or "music".
	player: String,
	/// Show the volume level (or "Muted") as the knob's title.
	show_level: bool,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			rotate: "volume".to_owned(),
			press: "mute".to_owned(),
			press_rotate: "none".to_owned(),
			volume_step: 5,
			seek_step: 10,
			reverse: false,
			player: "auto".to_owned(),
			show_level: true,
		}
	}
}

impl Settings {
	fn shows_level(&self) -> bool {
		self.show_level && (self.rotate == "volume" || self.press_rotate == "volume" || self.press == "mute")
	}
	fn player(&self) -> Player {
		Player::from_setting(&self.player)
	}
}

/// One thing to do. Turns arrive much faster than a volume command runs, so they are queued and merged.
#[derive(Debug, Clone, PartialEq)]
enum Command {
	Volume(i32),
	ToggleMute,
	PlayPause(Player),
	Next(Player),
	Previous(Player),
	Seek(Player, i64),
}

impl Command {
	/// Folds `next` into `self` when both are the same kind of turn, so ten quick ticks run as one command.
	fn merge(&mut self, next: &Command) -> bool {
		match (self, next) {
			(Command::Volume(a), Command::Volume(b)) => {
				*a += b;
				true
			}
			(Command::Seek(p, a), Command::Seek(q, b)) if p == q => {
				*a += b;
				true
			}
			_ => false,
		}
	}
}

static QUEUE: LazyLock<mpsc::UnboundedSender<Command>> = LazyLock::new(|| {
	let (tx, rx) = mpsc::unbounded_channel();
	tokio::spawn(worker(rx));
	tx
});

/// Settings of every knob currently shown, so level titles can be refreshed on all of them.
static SHOWN: LazyLock<Mutex<HashMap<String, Settings>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Per knob: is it held down, and was it turned while held (then releasing it is not a press).
#[derive(Default)]
struct Hold {
	down: bool,
	turned: bool,
}
static HOLD: LazyLock<Mutex<HashMap<String, Hold>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// A track skip per tick is far too fast for a knob; one skip per this interval is what a turn gives.
const TRACK_SETTLE: Duration = Duration::from_millis(350);
static LAST_TRACK: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

fn enqueue(command: Command) {
	let _ = QUEUE.send(command);
}

async fn worker(mut rx: mpsc::UnboundedReceiver<Command>) {
	let mut pending: Option<Command> = None;
	loop {
		let mut command = match pending.take() {
			Some(c) => c,
			None => match rx.recv().await {
				Some(c) => c,
				None => return,
			},
		};
		while let Ok(next) = rx.try_recv() {
			if !command.merge(&next) {
				pending = Some(next);
				break;
			}
		}
		log::debug!("Running {command:?}");
		let level = match &command {
			Command::Volume(0) | Command::Seek(_, 0) => continue,
			Command::Volume(delta) => media::change_volume(*delta).await,
			Command::ToggleMute => media::toggle_mute().await,
			Command::PlayPause(p) => {
				media::play_pause(*p).await;
				None
			}
			Command::Next(p) => {
				media::next(*p).await;
				None
			}
			Command::Previous(p) => {
				media::previous(*p).await;
				None
			}
			Command::Seek(p, seconds) => {
				media::seek(*p, *seconds).await;
				None
			}
		};
		if let Some(level) = level {
			show_level(level).await;
		}
	}
}

fn title_for(level: Level) -> String {
	if level.muted { "Muted".to_owned() } else { format!("{}%", level.percent) }
}

static LAST_LEVEL: LazyLock<Mutex<Option<Level>>> = LazyLock::new(|| Mutex::new(None));

async fn show_level(level: Level) {
	*LAST_LEVEL.lock().unwrap() = Some(level);
	let ids: Vec<String> = SHOWN.lock().unwrap().iter().filter(|(_, s)| s.shows_level()).map(|(id, _)| id.clone()).collect();
	for id in ids {
		if let Some(instance) = get_instance(id).await {
			let _ = instance.set_title(Some(title_for(level)), None).await;
		}
	}
}

/// Keeps level titles right when the volume is changed somewhere else (keyboard, menu bar).
async fn poll_level() {
	loop {
		tokio::time::sleep(Duration::from_secs(3)).await;
		let wanted = SHOWN.lock().unwrap().values().any(Settings::shows_level);
		if !wanted {
			continue;
		}
		if let Some(level) = media::level().await
			&& *LAST_LEVEL.lock().unwrap() != Some(level)
		{
			show_level(level).await;
		}
	}
}

fn turn(kind: &str, settings: &Settings, ticks: i16) {
	let ticks = if settings.reverse { -(ticks as i64) } else { ticks as i64 };
	let player = settings.player();
	match kind {
		"volume" => enqueue(Command::Volume(ticks as i32 * settings.volume_step.max(1) as i32)),
		"seek" => enqueue(Command::Seek(player, ticks * settings.seek_step.max(1) as i64)),
		"track" => {
			let mut last = LAST_TRACK.lock().unwrap();
			if last.is_some_and(|t| t.elapsed() < TRACK_SETTLE) {
				return;
			}
			*last = Some(Instant::now());
			enqueue(if ticks > 0 { Command::Next(player) } else { Command::Previous(player) });
		}
		_ => {}
	}
}

fn press(settings: &Settings) {
	let player = settings.player();
	match settings.press.as_str() {
		"playPause" => enqueue(Command::PlayPause(player)),
		"mute" => enqueue(Command::ToggleMute),
		"next" => enqueue(Command::Next(player)),
		"previous" => enqueue(Command::Previous(player)),
		_ => {}
	}
}

async fn refresh_title(instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
	if settings.shows_level() {
		let known = *LAST_LEVEL.lock().unwrap();
		let level = match known {
			Some(level) => Some(level),
			None => media::level().await,
		};
		if let Some(level) = level {
			*LAST_LEVEL.lock().unwrap() = Some(level);
			return instance.set_title(Some(title_for(level)), None).await;
		}
	}
	instance.set_title(None::<String>, None).await
}

struct KnobAction;
#[async_trait]
impl Action for KnobAction {
	const UUID: &'static str = "io.github.gauravk-in.mediaknob.knob";
	type Settings = Settings;

	async fn will_appear(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		SHOWN.lock().unwrap().insert(instance.instance_id.clone(), settings.clone());
		// A knob that was just dropped in the editor is not drawn yet, so a title set right away would be lost.
		let id = instance.instance_id.clone();
		let settings = settings.clone();
		tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(300)).await;
			if let Some(instance) = get_instance(id).await {
				let _ = refresh_title(&instance, &settings).await;
			}
		});
		Ok(())
	}

	async fn will_disappear(&self, instance: &Instance, _settings: &Settings) -> OpenActionResult<()> {
		SHOWN.lock().unwrap().remove(&instance.instance_id);
		HOLD.lock().unwrap().remove(&instance.instance_id);
		Ok(())
	}

	async fn did_receive_settings(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		SHOWN.lock().unwrap().insert(instance.instance_id.clone(), settings.clone());
		refresh_title(instance, settings).await
	}

	async fn dial_rotate(&self, instance: &Instance, settings: &Settings, ticks: i16, pressed: bool) -> OpenActionResult<()> {
		if ticks == 0 {
			return Ok(());
		}
		let held = {
			let mut hold = HOLD.lock().unwrap();
			let h = hold.entry(instance.instance_id.clone()).or_default();
			let held = pressed || h.down;
			if held && settings.press_rotate != "none" {
				h.turned = true;
			}
			held
		};
		let kind = if held && settings.press_rotate != "none" { &settings.press_rotate } else { &settings.rotate };
		turn(kind, settings, ticks);
		Ok(())
	}

	async fn dial_down(&self, instance: &Instance, _settings: &Settings) -> OpenActionResult<()> {
		HOLD.lock().unwrap().insert(instance.instance_id.clone(), Hold { down: true, turned: false });
		Ok(())
	}

	async fn dial_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		let turned = HOLD.lock().unwrap().remove(&instance.instance_id).is_some_and(|h| h.turned);
		if !turned {
			press(settings);
		}
		Ok(())
	}

	async fn key_up(&self, _instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		press(settings);
		Ok(())
	}
}

/// `opendeck-mediaknob --run <command> [amount]` runs one command without OpenDeck, for checking a machine.
async fn run_once(args: &[String]) {
	let amount: i64 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(5);
	let player = Player::from_setting(args.get(4).map(String::as_str).unwrap_or("auto"));
	match args.get(2).map(String::as_str).unwrap_or_default() {
		"level" => println!("{:?}", media::level().await),
		"volume" => println!("{:?}", media::change_volume(amount as i32).await),
		"mute" => println!("{:?}", media::toggle_mute().await),
		"playpause" => media::play_pause(player).await,
		"next" => media::next(player).await,
		"previous" => media::previous(player).await,
		"seek" => media::seek(player, amount).await,
		"players" => media::list_players().await,
		other => eprintln!("unknown command {other:?}; use level, volume N, mute, playpause, next, previous, seek N, players"),
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	let args: Vec<String> = std::env::args().collect();
	let debug = std::env::var_os("MEDIAKNOB_DEBUG").is_some();
	{
		use simplelog::*;
		let level = if debug { LevelFilter::Debug } else { LevelFilter::Info };
		if let Err(error) = TermLogger::init(level, Config::default(), TerminalMode::Stdout, ColorChoice::Never) {
			eprintln!("Logger initialization failed: {error}");
		}
	}

	if args.get(1).map(String::as_str) == Some("--run") {
		run_once(&args).await;
		return Ok(());
	}

	LazyLock::force(&QUEUE);
	tokio::spawn(poll_level());
	register_action(KnobAction).await;
	run(args).await
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn merges_turns_of_one_kind() {
		let mut c = Command::Volume(5);
		assert!(c.merge(&Command::Volume(5)));
		assert!(c.merge(&Command::Volume(-5)));
		assert_eq!(c, Command::Volume(5));
		assert!(!c.merge(&Command::ToggleMute));
		let mut s = Command::Seek(Player::Auto, 10);
		assert!(s.merge(&Command::Seek(Player::Auto, 10)));
		assert_eq!(s, Command::Seek(Player::Auto, 20));
		assert!(!s.merge(&Command::Seek(Player::Spotify, 10)));
		assert!(!Command::Next(Player::Auto).merge(&Command::Next(Player::Auto)));
	}

	#[test]
	fn old_or_partial_settings_load() {
		let s: Settings = serde_json::from_str(r#"{"rotate":"seek"}"#).unwrap();
		assert_eq!(s.rotate, "seek");
		assert_eq!(s.press, "mute");
		assert_eq!(s.volume_step, 5);
		assert!(!Settings { press: "playPause".into(), rotate: "track".into(), ..Default::default() }.shows_level());
	}
}
