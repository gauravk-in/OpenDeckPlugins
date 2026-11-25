mod deck;
mod demo;
mod ha;
mod layout;
mod model;
mod page;
mod render;
mod settings;
mod ui;

use openaction::*;

pub fn current_settings() -> settings::GlobalSettings {
	deck::APP.settings.read().unwrap().clone()
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	let args: Vec<String> = std::env::args().collect();
	// Debug helper: `opendeck-homeassistant --preview <dir>` draws the demo home's screens as PNGs.
	if args.get(1).map(String::as_str) == Some("--preview") {
		preview(args.get(2).map(String::as_str).unwrap_or("preview"));
		return Ok(());
	}

	// Debug helper: `opendeck-homeassistant --check <url> <token> [entity service]` connects, prints the rooms it
	// would show and optionally calls a service, without OpenDeck.
	if args.get(1).map(String::as_str) == Some("--check") {
		let _ = rustls::crypto::ring::default_provider().install_default();
		check(args[2..].to_vec()).await;
		return Ok(());
	}

	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Stdout, ColorChoice::Never) {
			eprintln!("Logger initialization failed: {error}");
		}
	}
	let _ = rustls::crypto::ring::default_provider().install_default();

	static EVENTS: deck::Events = deck::Events;
	global_events::set_global_event_handler(&EVENTS);
	register_action(deck::DeckAction).await;
	deck::spawn_loops();
	run(args).await
}

async fn check(args: Vec<String>) {
	use std::sync::{Arc, RwLock};
	let _ = simplelog::TermLogger::init(simplelog::LevelFilter::Info, simplelog::Config::default(), simplelog::TerminalMode::Stderr, simplelog::ColorChoice::Never);
	let settings = settings::GlobalSettings { url: args.first().cloned().unwrap_or_default(), token: args.get(1).cloned().unwrap_or_default(), ..Default::default() };
	*deck::APP.settings.write().unwrap() = settings.clone();
	let home = Arc::new(RwLock::new(model::Home { status: model::Status::Connecting, ..Default::default() }));
	let (tx, mut rx) = tokio::sync::broadcast::channel(64);
	let client = ha::start(settings, home.clone(), tx);
	let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
	loop {
		let status = home.read().unwrap().status;
		match status {
			model::Status::Ready => break,
			model::Status::AuthFailed | model::Status::NotConfigured => {
				eprintln!("status: {status:?}");
				std::process::exit(1);
			}
			_ => {}
		}
		if tokio::time::timeout_at(deadline, rx.recv()).await.is_err() {
			eprintln!("timed out, status: {status:?}");
			std::process::exit(1);
		}
	}
	{
		let h = home.read().unwrap();
		for room in &h.rooms {
			println!("{} [{}] {}", room.name, room.id, h.room_summary(room));
			for e in &room.entities {
				println!("  {:<40} {:<20} {}", e, h.display_name(e, Some(room)), h.state(e).map(|s| s.state.as_str()).unwrap_or("?"));
			}
		}
	}
	if let (Some(entity), Some(service)) = (args.get(2), args.get(3)) {
		client.call(ha::ServiceCall::entity(entity, service, serde_json::json!({})));
		// Wait for the state change to come back.
		let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
			while let Ok(c) = rx.recv().await {
				if let ha::Change::Entity(id) = c {
					if &id == entity {
						println!("{entity} -> {}", home.read().unwrap().state(entity).map(|s| s.state.clone()).unwrap_or_default());
						break;
					}
				}
			}
		})
		.await;
	}
}

/// Draws each screen of the demo home on a Streonor S6 into `<dir>/<screen>.png` (keys side by side, 3x3).
fn preview(dir: &str) {
	use std::time::{Duration, Instant};
	let _ = std::fs::create_dir_all(dir);
	let home = demo::home();
	let mut ui = ui::Ui::new(layout::Layout::streonor_s6());
	let now = Instant::now();
	let hold = Duration::from_millis(500);
	let shot = |ui: &ui::Ui, name: &str| {
		let views = ui.view(&home);
		let size = render::KEY_SIZE;
		let gap = 12;
		let cols = ui.layout.columns as u32;
		let rows = (views.len() as u32).div_ceil(cols);
		let mut sheet = image::RgbaImage::from_pixel(cols * (size + gap) + gap, rows * (size + gap) + gap, image::Rgba([60, 60, 66, 255]));
		for (i, v) in views.iter().enumerate() {
			let img = if ui.layout.display[i] { render::key(v) } else { plain_button(v) };
			let (x, y) = (gap + (i as u32 % cols) * (size + gap), gap + (i as u32 / cols) * (size + gap));
			image::imageops::overlay(&mut sheet, &img, x as i64, y as i64);
		}
		let path = format!("{dir}/{name}.png");
		std::fs::write(&path, render::to_png(&sheet)).expect("write preview");
		println!("{path}");
	};
	shot(&ui, "1-rooms");
	ui.dial_rotate(0, 1, now, &home);
	shot(&ui, "2-rooms-knob-focus");
	ui.dial_press(0, now, &home);
	shot(&ui, "3-living-room");
	ui.key_down(1, now);
	ui.key_up(1, now, hold, &home);
	ui.dial_rotate(0, 3, now, &home);
	shot(&ui, "4-thermostat-turning");
	ui.go_home();
	ui.key_down(0, now);
	ui.key_up(0, now, hold, &home);
	ui.key_down(3, now);
	ui.tick(now + Duration::from_secs(1), hold, None, &home);
	shot(&ui, "5-lamp");
}

/// The S6's bottom row has no screens; show what each button does in the preview.
fn plain_button(v: &ui::KeyView) -> image::RgbaImage {
	match v {
		ui::KeyView::Nav(..) => render::key(v),
		_ => render::key(&ui::KeyView::Blank),
	}
}
