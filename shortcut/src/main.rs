mod action;
mod keys;
mod render;
mod shortcut;

use openaction::*;

fn usage() -> ! {
	eprintln!(
		"usage: opendeck-shortcut --check\n       opendeck-shortcut --send '<shortcut>' [delay seconds]\n       \
		 opendeck-shortcut --image '<shortcut>' out.png"
	);
	std::process::exit(2)
}

/// Debug helpers that run without OpenDeck. Returns true when one ran.
fn cli(args: &[String]) -> bool {
	match args.get(1).map(String::as_str) {
		Some("--check") => {
			match keys::ready() {
				Ok(()) => println!("Ready to send keys ({})", keys::backend()),
				Err(e) => {
					println!("Not ready: {e}");
					std::process::exit(1);
				}
			}
			true
		}
		Some("--send") => {
			let Some(text) = args.get(2) else { usage() };
			let s = shortcut::parse(text).unwrap_or_else(|e| {
				eprintln!("{e}");
				std::process::exit(2)
			});
			let delay: f64 = args.get(3).and_then(|d| d.parse().ok()).unwrap_or(0.0);
			std::thread::sleep(std::time::Duration::from_secs_f64(delay));
			println!("Sending {} ({})", s.canonical(), s.label(action::MAC));
			if let Err(e) = keys::tap(&s, 1) {
				eprintln!("{e}");
				std::process::exit(1);
			}
			true
		}
		Some("--image") => {
			let (Some(text), Some(out)) = (args.get(2), args.get(3)) else { usage() };
			let label = shortcut::parse(text).map(|s| s.label(action::MAC)).unwrap_or_else(|_| text.clone());
			std::fs::write(out, render::to_png(&render::shortcut_image(&label))).expect("write png");
			true
		}
		Some("--help") | Some("-h") => usage(),
		_ => false,
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	let args: Vec<String> = std::env::args().collect();
	if cli(&args) {
		return Ok(());
	}
	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Stdout, ColorChoice::Never) {
			eprintln!("Logger initialization failed: {error}");
		}
	}
	register_action(action::ShortcutAction).await;
	run(args).await
}
