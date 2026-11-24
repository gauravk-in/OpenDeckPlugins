mod action;
mod browser;
mod logo;
mod render;

use openaction::*;

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	// Debug helper: `opendeck-weblink --probe <url> [out.png]` prints where the logo comes from.
	let args: Vec<String> = std::env::args().collect();
	if args.get(1).map(String::as_str) == Some("--profiles") {
		for p in browser::list_profiles() {
			println!("{}\t{}", p.value, p.label);
		}
		return Ok(());
	}
	if args.get(1).map(String::as_str) == Some("--probe") {
		probe(args.get(2).cloned().unwrap_or_default(), args.get(3).cloned(), args.get(4).cloned()).await;
		return Ok(());
	}

	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Stdout, ColorChoice::Never) {
			eprintln!("Logger initialization failed: {error}");
		}
	}

	register_action(action::WebLinkAction).await;
	run(args).await
}

async fn probe(site: String, out: Option<String>, caption: Option<String>) {
	let _ = simplelog::TermLogger::init(
		simplelog::LevelFilter::Debug,
		simplelog::ConfigBuilder::new().add_filter_allow_str("opendeck_weblink").build(),
		simplelog::TerminalMode::Stderr,
		simplelog::ColorChoice::Never,
	);
	let Some(url) = logo::normalise_url(&site) else {
		eprintln!("usage: opendeck-weblink --probe <url> [out.png] [caption]");
		std::process::exit(2);
	};
	let client = logo::client();
	let img = match logo::discover(&client, &url, true).await {
		Some(l) => {
			println!("{url}\t{}\t{}x{}", l.source, l.image.width(), l.image.height());
			render::compose(&l.image, "auto", 0.12)
		}
		None => {
			println!("{url}\tNONE\tletter tile");
			render::letter_tile(url.host_str().unwrap_or("?"), "")
		}
	};
	let img = render::add_caption(&img, caption.as_deref().unwrap_or(""), "auto");
	if let Some(out) = out {
		std::fs::write(out, render::to_png(&img)).expect("write png");
	}
}
