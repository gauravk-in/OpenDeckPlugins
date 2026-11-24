mod pages;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use openaction::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
	/// Past the last page, continue from the first (and the other way round).
	wrap: bool,
	/// Only cycle through pages in the current page's folder.
	same_folder: bool,
	/// Swap the turning direction of the dial.
	reverse: bool,
	/// What pressing the dial does: "first" (go to the first page) or "none".
	press: String,
	/// Show the name of the page a Next/Previous key leads to as its title.
	show_name: bool,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			wrap: true,
			same_folder: false,
			reverse: false,
			press: "first".to_owned(),
			show_name: true,
		}
	}
}

#[derive(Serialize)]
struct SwitchProfileEvent<'a> {
	event: &'static str,
	device: &'a str,
	profile: &'a str,
}

/// The page an instance sits on, from its context (`device.profile.controller.position.index`).
fn current_page(instance: &Instance) -> Option<String> {
	instance.instance_id.split('.').nth(1).map(str::to_owned)
}

fn target(instance: &Instance, settings: &Settings, steps: i64) -> Option<String> {
	let config = pages::config_dir()?;
	let all = pages::device_pages(&config, &instance.device_id);
	pages::step(&all, &current_page(instance)?, steps, settings.wrap, settings.same_folder)
}

/// Turning a dial quickly sends several events before the new page has appeared; they would all be sent from
/// the old page and land on the same target. Ignore turns for a moment after each switch.
static LAST_SWITCH: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const SETTLE: Duration = Duration::from_millis(200);
const TITLE_DELAY: Duration = Duration::from_millis(300);

async fn switch_to(instance: &Instance, page: &str) -> OpenActionResult<()> {
	LAST_SWITCH.lock().unwrap().insert(instance.device_id.clone(), Instant::now());
	log::info!("Switching {} to page {page}", instance.device_id);
	send_arbitrary_json(SwitchProfileEvent {
		event: "switchProfile",
		device: &instance.device_id,
		profile: page,
	})
	.await
}

fn settling(device: &str) -> bool {
	LAST_SWITCH.lock().unwrap().get(device).is_some_and(|t| t.elapsed() < SETTLE)
}

async fn step(instance: &Instance, settings: &Settings, steps: i64) -> OpenActionResult<()> {
	match target(instance, settings, steps) {
		Some(page) => switch_to(instance, &page).await,
		None => Ok(()),
	}
}

async fn show_target(instance: &Instance, settings: &Settings, steps: i64) -> OpenActionResult<()> {
	if instance.controller != "Keypad" {
		return Ok(());
	}
	let page = target(instance, settings, steps);
	log::debug!("{} on {:?}: leads to {:?}", instance.instance_id, current_page(instance), page);
	match page.filter(|_| settings.show_name) {
		Some(page) => instance.set_title(Some(pages::display_name(&page)), None).await,
		None => instance.set_title(None::<String>, None).await,
	}
}

/// A key that was just dropped in the editor is not drawn yet, so a title set right away would be lost.
fn show_target_later(instance: &Instance, settings: &Settings, steps: i64) {
	let id = instance.instance_id.clone();
	let settings = settings.clone();
	tokio::spawn(async move {
		tokio::time::sleep(TITLE_DELAY).await;
		if let Some(instance) = get_instance(id).await {
			let _ = show_target(&instance, &settings, steps).await;
		}
	});
}

/// Dial: turn for next/previous page, press for the first page.
struct DialAction;
#[async_trait]
impl Action for DialAction {
	const UUID: &'static str = "io.github.gauravk-in.pageswitcher.dial";
	type Settings = Settings;

	async fn dial_rotate(&self, instance: &Instance, settings: &Settings, ticks: i16, _pressed: bool) -> OpenActionResult<()> {
		if ticks == 0 || settling(&instance.device_id) {
			return Ok(());
		}
		let direction = if (ticks > 0) != settings.reverse { 1 } else { -1 };
		step(instance, settings, direction).await
	}

	async fn dial_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		self.key_up(instance, settings).await
	}

	async fn key_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		if settings.press != "first" {
			return Ok(());
		}
		let Some(config) = pages::config_dir() else { return Ok(()) };
		let all = pages::device_pages(&config, &instance.device_id);
		let current = current_page(instance).unwrap_or_default();
		let first = if settings.same_folder {
			let folder = current.split_once('/').map(|(f, _)| f).unwrap_or("");
			all.iter().find(|p| p.split_once('/').map(|(f, _)| f).unwrap_or("") == folder)
		} else {
			all.first()
		};
		match first {
			Some(page) if *page != current => switch_to(instance, page).await,
			_ => Ok(()),
		}
	}
}

struct NextAction;
#[async_trait]
impl Action for NextAction {
	const UUID: &'static str = "io.github.gauravk-in.pageswitcher.next";
	type Settings = Settings;

	async fn will_appear(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		show_target_later(instance, settings, 1);
		Ok(())
	}
	async fn did_receive_settings(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		show_target(instance, settings, 1).await
	}
	async fn key_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		step(instance, settings, 1).await
	}
	async fn dial_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		step(instance, settings, 1).await
	}
}

struct PreviousAction;
#[async_trait]
impl Action for PreviousAction {
	const UUID: &'static str = "io.github.gauravk-in.pageswitcher.previous";
	type Settings = Settings;

	async fn will_appear(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		show_target_later(instance, settings, -1);
		Ok(())
	}
	async fn did_receive_settings(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		show_target(instance, settings, -1).await
	}
	async fn key_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		step(instance, settings, -1).await
	}
	async fn dial_up(&self, instance: &Instance, settings: &Settings) -> OpenActionResult<()> {
		step(instance, settings, -1).await
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	let args: Vec<String> = std::env::args().collect();
	// Debug helper: `opendeck-pageswitcher --pages <device>` lists that device's pages in order.
	if args.get(1).map(String::as_str) == Some("--pages") {
		let config = pages::config_dir().expect("set OPENDECK_CONFIG_DIR to OpenDeck's config folder");
		for page in pages::device_pages(&config, args.get(2).map(String::as_str).unwrap_or_default()) {
			println!("{page}");
		}
		return Ok(());
	}

	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Stdout, ColorChoice::Never) {
			eprintln!("Logger initialization failed: {error}");
		}
	}

	register_action(DialAction).await;
	register_action(NextAction).await;
	register_action(PreviousAction).await;
	run(args).await
}
