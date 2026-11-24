use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use image::DynamicImage;
use log::{info, warn};
use openaction::*;
use serde::{Deserialize, Serialize};

use crate::{logo, render};

fn default_true() -> bool {
	true
}
fn default_background() -> String {
	"auto".into()
}
fn default_padding() -> u32 {
	12
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
	/// Website to open.
	pub url: String,
	/// Text drawn under a smaller logo (e.g. to tell two links to the same site apart).
	pub caption: String,
	/// "" for the default browser, or "<browser>:<profile dir>" such as "chrome:Profile 1".
	pub open_with: String,
	/// Optional image URL that overrides the discovered logo.
	pub logo_url: String,
	/// "auto", "black", "white" or "#rrggbb".
	#[serde(default = "default_background")]
	pub background: String,
	/// Empty space around non-square logos, in percent of the key.
	#[serde(default = "default_padding")]
	pub padding: u32,
	/// Set the logo as the key image at all.
	#[serde(default = "default_true")]
	pub show_logo: bool,
	/// Ask Google's favicon service when the site itself offers nothing usable.
	#[serde(default = "default_true")]
	pub online_fallback: bool,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			url: String::new(),
			caption: String::new(),
			open_with: String::new(),
			logo_url: String::new(),
			background: default_background(),
			padding: default_padding(),
			show_logo: true,
			online_fallback: true,
		}
	}
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(logo::client);
/// Latest render request per instance, so slow downloads can't overwrite newer settings.
static GENERATION: LazyLock<DashMap<InstanceId, u64>> = LazyLock::new(DashMap::new);
static NEXT_GEN: AtomicU64 = AtomicU64::new(1);
/// Settings each instance was last rendered with.
static LAST: LazyLock<DashMap<InstanceId, Settings>> = LazyLock::new(DashMap::new);
/// Sites where we found nothing recently; avoids hammering them on every settings change.
static MISSES: LazyLock<DashMap<String, Instant>> = LazyLock::new(DashMap::new);
/// Last preview sent, so the property inspector can show it when opened.
static PREVIEW: LazyLock<DashMap<InstanceId, serde_json::Value>> = LazyLock::new(DashMap::new);

const MISS_TTL: Duration = Duration::from_secs(60 * 60);
const CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

fn cache_dir() -> PathBuf {
	dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("opendeck-weblink")
}

fn cache_path(key: &str) -> PathBuf {
	cache_dir().join(format!("{}.png", sha1_smol::Sha1::from(key.as_bytes()).digest()))
}

fn load_cached(key: &str) -> Option<(DynamicImage, String)> {
	let path = cache_path(key);
	let meta = std::fs::metadata(&path).ok()?;
	if meta.modified().ok()?.elapsed().unwrap_or_default() > CACHE_TTL {
		return None;
	}
	let img = image::open(&path).ok()?;
	let source = std::fs::read_to_string(path.with_extension("txt")).unwrap_or_else(|_| "cache".into());
	Some((img, source))
}

fn store_cached(key: &str, img: &DynamicImage, source: &str) {
	let path = cache_path(key);
	if let Err(e) = std::fs::create_dir_all(cache_dir()) {
		warn!("cannot create cache dir: {e}");
		return;
	}
	// Keep the cached copy small; the key is 144px anyway.
	let img = if img.width().max(img.height()) > 288 { img.resize(288, 288, image::imageops::FilterType::Lanczos3) } else { img.clone() };
	if let Err(e) = img.to_rgba8().save(&path) {
		warn!("cannot write cache {}: {e}", path.display());
	}
	let _ = std::fs::write(path.with_extension("txt"), source);
}

fn forget_cached(key: &str) {
	let path = cache_path(key);
	let _ = std::fs::remove_file(&path);
	let _ = std::fs::remove_file(path.with_extension("txt"));
	MISSES.remove(key);
}

/// What we look up: the custom logo URL if given, otherwise the site.
fn lookup_key(settings: &Settings) -> Option<(String, url::Url, bool)> {
	if !settings.logo_url.trim().is_empty()
		&& let Some(u) = logo::normalise_url(&settings.logo_url)
	{
		return Some((format!("direct:{u}"), u, true));
	}
	let u = logo::normalise_url(&settings.url)?;
	Some((format!("site:{u}"), u, false))
}

async fn get_logo(settings: &Settings, force: bool) -> (Option<DynamicImage>, String) {
	let Some((key, url, direct)) = lookup_key(settings) else { return (None, String::new()) };
	if force {
		forget_cached(&key);
	} else {
		if let Some((img, source)) = load_cached(&key) {
			return (Some(img), source);
		}
		if MISSES.get(&key).is_some_and(|t| t.elapsed() < MISS_TTL) {
			return (None, "no logo found (cached)".into());
		}
	}
	if !matches!(url.scheme(), "http" | "https") {
		return (None, format!("no logo for {}: links", url.scheme()));
	}
	let found = if direct { logo::fetch_direct(&CLIENT, &url).await } else { logo::discover(&CLIENT, &url, settings.online_fallback).await };
	match found {
		Some(l) => {
			store_cached(&key, &l.image, &l.source);
			(Some(l.image), l.source)
		}
		None => {
			MISSES.insert(key, Instant::now());
			(None, "no logo found".into())
		}
	}
}

fn site_label(settings: &Settings) -> String {
	logo::normalise_url(&settings.url)
		.and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_owned()).or_else(|| Some(u.scheme().to_owned())))
		.unwrap_or_default()
}

/// Works out the key image in the background and applies it if nothing newer was requested meanwhile.
fn refresh(instance_id: InstanceId, settings: Settings, force: bool) {
	let generation = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
	GENERATION.insert(instance_id.clone(), generation);
	LAST.insert(instance_id.clone(), settings.clone());

	tokio::spawn(async move {
		let is_current = || GENERATION.get(&instance_id).is_some_and(|g| *g == generation);
		let Some(instance) = get_instance(instance_id.clone()).await else { return };

		if !settings.show_logo || (settings.url.trim().is_empty() && settings.logo_url.trim().is_empty()) {
			let _ = instance.set_image(None::<String>, None).await;
			let status = if settings.show_logo { "Enter a website address" } else { "Logo turned off" };
			send_preview(&instance, serde_json::json!({ "status": status })).await;
			return;
		}

		if !force {
			send_preview(&instance, serde_json::json!({ "status": "Looking for the logo…" })).await;
		}
		let (found, source) = get_logo(&settings, force).await;
		if !is_current() {
			return;
		}
		let (img, status) = match found {
			Some(img) => {
				// With a caption the whole tile is shrunk, so use less inner padding to keep the logo readable.
				let padding = settings.padding as f32 / 100.0 * if settings.caption.trim().is_empty() { 1.0 } else { 0.5 };
				(render::compose(&img, &settings.background, padding), format!("Logo from {source}"))
			}
			None => {
				let bg = if settings.background.starts_with('#') { settings.background.as_str() } else { "" };
				(render::letter_tile(&site_label(&settings), bg), format!("{source}, using a letter tile"))
			}
		};
		let img = render::add_caption(&img, &settings.caption, &settings.background);
		let data = render::data_url(&render::to_png(&img));
		if let Err(e) = instance.set_image(Some(data.clone()), None).await {
			warn!("set_image failed: {e}");
		}
		info!("{}: {status}", settings.url);
		send_preview(&instance, serde_json::json!({ "status": status, "preview": data })).await;
	});
}

async fn send_preview(instance: &Instance, payload: serde_json::Value) {
	PREVIEW.insert(instance.instance_id.clone(), payload.clone());
	let _ = instance.send_to_property_inspector(payload).await;
}

pub struct WebLinkAction;

impl WebLinkAction {
	async fn open(settings: &Settings) -> OpenActionResult<()> {
		let Some(url) = logo::normalise_url(&settings.url) else { return Ok(()) };
		let url = url.to_string();
		if !settings.open_with.trim().is_empty() && crate::browser::open_in_profile(settings.open_with.trim(), &url).await {
			return Ok(());
		}
		open_url(url).await?;
		Ok(())
	}
}

#[async_trait]
impl Action for WebLinkAction {
	const UUID: &'static str = "io.github.gauravk-in.weblink.open";
	type Settings = Settings;

	async fn will_appear(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		refresh(instance.instance_id.clone(), settings.clone(), false);
		Ok(())
	}

	async fn will_disappear(&self, instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
		GENERATION.remove(&instance.instance_id);
		LAST.remove(&instance.instance_id);
		PREVIEW.remove(&instance.instance_id);
		Ok(())
	}

	async fn did_receive_settings(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		let changed = LAST.get(&instance.instance_id).is_none_or(|old| *old != *settings);
		if changed {
			refresh(instance.instance_id.clone(), settings.clone(), false);
		}
		Ok(())
	}

	async fn property_inspector_did_appear(&self, instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
		let profiles = tokio::task::spawn_blocking(crate::browser::list_profiles).await.unwrap_or_default();
		instance.send_to_property_inspector(serde_json::json!({ "profiles": profiles })).await?;
		if let Some(p) = PREVIEW.get(&instance.instance_id).map(|p| p.clone()) {
			instance.send_to_property_inspector(p).await?;
		}
		Ok(())
	}

	async fn send_to_plugin(&self, instance: &Instance, settings: &Self::Settings, payload: &serde_json::Value) -> OpenActionResult<()> {
		if payload.get("action").and_then(|a| a.as_str()) == Some("refresh") {
			send_preview(instance, serde_json::json!({ "status": "Fetching the logo again…" })).await;
			refresh(instance.instance_id.clone(), settings.clone(), true);
		}
		Ok(())
	}

	async fn key_up(&self, _instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		Self::open(settings).await
	}

	async fn dial_up(&self, _instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		Self::open(settings).await
	}
}
