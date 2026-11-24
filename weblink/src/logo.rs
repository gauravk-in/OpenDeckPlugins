//! Finds and downloads the best logo for a website.
//!
//! Order of preference:
//! 1. `<link rel="apple-touch-icon">` (designed as a square tile, looks best on a key)
//! 2. Web app manifest icons (`<link rel="manifest">`)
//! 3. `<link rel="icon">` / `shortcut icon` (largest first, SVG counts as large)
//! 4. Well-known paths: `/apple-touch-icon.png`, `/favicon.ico`
//! 5. Google's favicon service (optional, for sites that hide their icons)
//!
//! Anything that fails to download or decode is skipped. If nothing works the caller
//! draws a letter tile instead.

use std::time::Duration;

use image::{DynamicImage, GenericImageView};
use log::{debug, info};
use scraper::{Html, Selector};
use url::Url;

const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// A logo at least this large is good enough to stop searching.
const GOOD_ENOUGH: u32 = 96;

pub struct Logo {
	pub image: DynamicImage,
	pub source: String,
}

#[derive(Debug, Clone)]
struct Candidate {
	url: Url,
	/// Declared size in pixels (0 = unknown). SVGs are treated as 512.
	size: u32,
	/// Lower is better.
	rank: u8,
}

pub fn client() -> reqwest::Client {
	reqwest::Client::builder()
		.user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15")
		.timeout(Duration::from_secs(12))
		.connect_timeout(Duration::from_secs(6))
		// Home-automation dashboards (Home Assistant, routers, NAS…) often use self-signed
		// certificates. We only download pictures here, never send anything, so accept them.
		.danger_accept_invalid_certs(true)
		.redirect(reqwest::redirect::Policy::limited(8))
		.build()
		.expect("failed to build HTTP client")
}

/// Turns what the user typed into a proper URL ("github.com" -> "https://github.com/").
pub fn normalise_url(input: &str) -> Option<Url> {
	let input = input.trim();
	if input.is_empty() {
		return None;
	}
	// Keep explicit schemes ("http://…", "mailto:…", "spotify:…"), but treat "host:port" as a host.
	let has_scheme = input.contains("://")
		|| input.split_once(':').is_some_and(|(scheme, rest)| {
			!scheme.is_empty()
				&& scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
				&& !scheme.contains('.')
				&& !scheme.eq_ignore_ascii_case("localhost")
				&& !rest.starts_with(|c: char| c.is_ascii_digit())
		});
	let with_scheme = if has_scheme { input.to_owned() } else { format!("https://{input}") };
	Url::parse(&with_scheme).ok()
}

async fn get_bytes(client: &reqwest::Client, url: &Url, limit: usize) -> Option<(Url, Option<String>, Vec<u8>)> {
	let mut resp = client.get(url.clone()).send().await.ok()?;
	if !resp.status().is_success() {
		debug!("{url}: HTTP {}", resp.status());
		return None;
	}
	let final_url = resp.url().clone();
	let content_type = resp
		.headers()
		.get(reqwest::header::CONTENT_TYPE)
		.and_then(|v| v.to_str().ok())
		.map(|s| s.to_ascii_lowercase());
	let mut body = Vec::new();
	while let Some(chunk) = resp.chunk().await.ok()? {
		body.extend_from_slice(&chunk);
		if body.len() > limit {
			debug!("{url}: response too large");
			return None;
		}
	}
	Some((final_url, content_type, body))
}

fn parse_sizes(sizes: Option<&str>) -> u32 {
	let Some(sizes) = sizes else { return 0 };
	if sizes.trim().eq_ignore_ascii_case("any") {
		return 512;
	}
	sizes
		.split_whitespace()
		.filter_map(|s| s.to_ascii_lowercase().split('x').next().and_then(|w| w.parse::<u32>().ok()))
		.max()
		.unwrap_or(0)
}

fn is_svg(url: &Url, mime: Option<&str>) -> bool {
	mime.is_some_and(|m| m.contains("svg")) || url.path().to_ascii_lowercase().ends_with(".svg")
}

/// Pulls icon candidates out of a page's HTML. Returns (candidates, manifest URL).
fn candidates_from_html(html: &str, page_url: &Url) -> (Vec<Candidate>, Option<Url>) {
	let doc = Html::parse_document(html);
	let base = Selector::parse("base[href]")
		.ok()
		.and_then(|sel| doc.select(&sel).next())
		.and_then(|el| el.value().attr("href"))
		.and_then(|href| page_url.join(href).ok())
		.unwrap_or_else(|| page_url.clone());

	let mut out = Vec::new();
	let mut manifest = None;
	let link_sel = Selector::parse("link[rel][href]").unwrap();
	for el in doc.select(&link_sel) {
		let rel = el.value().attr("rel").unwrap_or_default().to_ascii_lowercase();
		let Some(href) = el.value().attr("href") else { continue };
		let Ok(url) = base.join(href.trim()) else { continue };
		let mime = el.value().attr("type").map(|t| t.to_ascii_lowercase());
		let rels: Vec<&str> = rel.split_whitespace().collect();

		if rels.contains(&"manifest") {
			manifest.get_or_insert(url);
			continue;
		}
		let declared = parse_sizes(el.value().attr("sizes"));
		if rels.iter().any(|r| r.starts_with("apple-touch-icon")) {
			out.push(Candidate { url, size: if declared == 0 { 180 } else { declared }, rank: 0 });
		} else if rels.contains(&"icon") || rels.contains(&"shortcut") {
			let svg = is_svg(&url, mime.as_deref());
			out.push(Candidate { url, size: if svg { 512 } else { declared }, rank: 2 });
		}
		// rel="mask-icon" is a single-colour silhouette meant for Safari pinned tabs; skip it.
	}

	// Some sites only advertise an image via og:image/msapplication; use the tile image as a last resort.
	if let Ok(sel) = Selector::parse(r#"meta[name="msapplication-TileImage"][content]"#)
		&& let Some(content) = doc.select(&sel).next().and_then(|el| el.value().attr("content"))
		&& let Ok(url) = base.join(content.trim())
	{
		out.push(Candidate { url, size: 144, rank: 3 });
	}
	(out, manifest)
}

#[derive(serde::Deserialize)]
struct Manifest {
	#[serde(default)]
	icons: Vec<ManifestIcon>,
}
#[derive(serde::Deserialize)]
struct ManifestIcon {
	src: String,
	sizes: Option<String>,
	#[serde(rename = "type")]
	mime: Option<String>,
	purpose: Option<String>,
}

async fn candidates_from_manifest(client: &reqwest::Client, manifest_url: &Url) -> Vec<Candidate> {
	let Some((final_url, _, body)) = get_bytes(client, manifest_url, MAX_HTML_BYTES).await else { return vec![] };
	let Ok(manifest) = serde_json::from_slice::<Manifest>(&body) else { return vec![] };
	manifest
		.icons
		.into_iter()
		.filter(|icon| !icon.purpose.as_deref().unwrap_or("any").contains("monochrome"))
		.filter_map(|icon| {
			let url = final_url.join(&icon.src).ok()?;
			let svg = is_svg(&url, icon.mime.as_deref());
			let size = if svg { 512 } else { parse_sizes(icon.sizes.as_deref()) };
			Some(Candidate { url, size, rank: 1 })
		})
		.collect()
}

pub fn decode_image(bytes: &[u8], mime: Option<&str>, url: &Url) -> Option<DynamicImage> {
	let looks_like_svg = is_svg(url, mime) || {
		let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
		head.contains("<svg") || (head.starts_with("<?xml") && head.contains("svg"))
	};
	if looks_like_svg {
		return render_svg(bytes);
	}
	image::load_from_memory(bytes).ok()
}

fn render_svg(bytes: &[u8]) -> Option<DynamicImage> {
	let opts = resvg::usvg::Options::default();
	let tree = resvg::usvg::Tree::from_data(bytes, &opts).ok()?;
	let size = tree.size();
	let target = 256.0_f32;
	let scale = target / size.width().max(size.height()).max(1.0);
	let w = (size.width() * scale).ceil().max(1.0) as u32;
	let h = (size.height() * scale).ceil().max(1.0) as u32;
	let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
	resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
	// tiny-skia stores premultiplied RGBA; convert to straight alpha.
	let mut data = pixmap.take();
	for px in data.chunks_exact_mut(4) {
		let a = px[3] as u32;
		if a > 0 && a < 255 {
			for c in &mut px[..3] {
				*c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
			}
		}
	}
	image::RgbaImage::from_raw(w, h, data).map(DynamicImage::ImageRgba8)
}

async fn try_candidate(client: &reqwest::Client, c: &Candidate) -> Option<DynamicImage> {
	let (final_url, mime, bytes) = get_bytes(client, &c.url, MAX_IMAGE_BYTES).await?;
	if mime.as_deref().is_some_and(|m| m.starts_with("text/html")) {
		return None; // soft 404 page
	}
	let img = decode_image(&bytes, mime.as_deref(), &final_url)?;
	let (w, h) = img.dimensions();
	if w < 8 || h < 8 {
		return None;
	}
	Some(img)
}

/// Downloads a specific image URL (used when the user sets a custom logo URL).
pub async fn fetch_direct(client: &reqwest::Client, url: &Url) -> Option<Logo> {
	let c = Candidate { url: url.clone(), size: 0, rank: 0 };
	try_candidate(client, &c).await.map(|image| Logo { image, source: url.to_string() })
}

/// Finds the best logo for the website at `site`.
pub async fn discover(client: &reqwest::Client, site: &Url, use_online_service: bool) -> Option<Logo> {
	let mut candidates = Vec::new();
	let mut origin = site.clone();
	origin.set_path("/");
	origin.set_query(None);
	origin.set_fragment(None);

	// The page itself, and the site root if the URL points deeper (deep links often lack <link> tags).
	let mut pages = vec![site.clone()];
	if site.path() != "/" {
		pages.push(origin.clone());
	}
	for page in pages {
		let Some((final_url, mime, body)) = get_bytes(client, &page, MAX_HTML_BYTES).await else { continue };
		if !mime.as_deref().unwrap_or("text/html").contains("html") {
			continue;
		}
		origin = final_url.clone();
		origin.set_path("/");
		origin.set_query(None);
		let html = String::from_utf8_lossy(&body);
		let (mut found, manifest) = candidates_from_html(&html, &final_url);
		if let Some(manifest) = manifest {
			found.extend(candidates_from_manifest(client, &manifest).await);
		}
		if !found.is_empty() {
			candidates = found;
			break;
		}
	}

	// Well-known fallbacks on whichever origin we ended up on (after redirects).
	for (path, size) in [("/apple-touch-icon.png", 180), ("/favicon.ico", 0)] {
		if let Ok(url) = origin.join(path) {
			candidates.push(Candidate { url, size, rank: 4 });
		}
	}
	if use_online_service && let Some(host) = origin.host_str() {
		if let Ok(url) = Url::parse(&format!("https://www.google.com/s2/favicons?domain={host}&sz=256")) {
			candidates.push(Candidate { url, size: 0, rank: 5 });
		}
	}

	// Best first: by rank, then larger declared size. Prefer >=96px; ignore huge-but-unknown ordering.
	candidates.sort_by(|a, b| a.rank.cmp(&b.rank).then(b.size.cmp(&a.size)));
	candidates.dedup_by(|a, b| a.url == b.url);

	let mut best: Option<Logo> = None;
	for c in &candidates {
		debug!("trying {} (rank {}, size {})", c.url, c.rank, c.size);
		let Some(img) = try_candidate(client, c).await else { continue };
		let px = img.width().min(img.height());
		let better = best.as_ref().is_none_or(|b| px > b.image.width().min(b.image.height()));
		if better {
			best = Some(Logo { image: img, source: c.url.to_string() });
		}
		if px >= GOOD_ENOUGH {
			break;
		}
	}
	if let Some(b) = &best {
		info!("logo for {site}: {} ({}x{})", b.source, b.image.width(), b.image.height());
	}
	best
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_links() {
		let html = r#"<html><head>
			<link rel="icon" href="/favicon-32.png" sizes="32x32">
			<link rel="icon" type="image/svg+xml" href="icon.svg">
			<link rel="apple-touch-icon" href="https://cdn.example.com/touch.png">
			<link rel="manifest" href="/site.webmanifest">
			<link rel="mask-icon" href="/mask.svg">
		</head></html>"#;
		let page = Url::parse("https://example.com/a/b").unwrap();
		let (c, m) = candidates_from_html(html, &page);
		assert_eq!(m.unwrap().as_str(), "https://example.com/site.webmanifest");
		assert_eq!(c.len(), 3);
		assert!(c.iter().any(|c| c.url.as_str() == "https://example.com/a/icon.svg" && c.size == 512));
		assert!(c.iter().any(|c| c.rank == 0 && c.size == 180));
	}

	#[test]
	fn normalises() {
		assert_eq!(normalise_url("github.com").unwrap().as_str(), "https://github.com/");
		assert_eq!(normalise_url("http://192.168.1.2:8123").unwrap().as_str(), "http://192.168.1.2:8123/");
		assert!(normalise_url("  ").is_none());
		assert_eq!(normalise_url("localhost:8123").unwrap().as_str(), "https://localhost:8123/");
		assert_eq!(normalise_url("homeassistant.local:8123").unwrap().as_str(), "https://homeassistant.local:8123/");
		assert_eq!(normalise_url("mailto:me@example.com").unwrap().scheme(), "mailto");
	}

	#[test]
	fn sizes() {
		assert_eq!(parse_sizes(Some("16x16 32x32 192x192")), 192);
		assert_eq!(parse_sizes(Some("any")), 512);
		assert_eq!(parse_sizes(None), 0);
	}
}
