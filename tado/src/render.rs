//! Draws the room keys: room name, current temperature, target temperature and a heating bar.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use base64::Engine;
use image::{Rgba, RgbaImage};

use crate::api::RoomState;

pub const KEY_SIZE: u32 = 144;
static FONT: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold.ttf");

const BG: Rgba<u8> = Rgba([18, 18, 20, 255]);
const BG_SELECTED: Rgba<u8> = Rgba([58, 34, 10, 255]);
const ACCENT: Rgba<u8> = Rgba([255, 150, 40, 255]);
const WHITE: Rgba<u8> = Rgba([245, 245, 245, 255]);
const MUTED: Rgba<u8> = Rgba([160, 160, 165, 255]);
const PENDING: Rgba<u8> = Rgba([255, 214, 102, 255]);
const COLD: Rgba<u8> = Rgba([120, 180, 255, 255]);

pub struct Key<'a> {
	pub name: &'a str,
	pub state: Option<&'a RoomState>,
	pub selected: bool,
	/// A target the user is still turning to, not sent yet.
	pub pending: Option<Option<f64>>,
	/// Shown instead of the target line, e.g. "Sign in".
	pub notice: Option<&'a str>,
}

fn font() -> FontRef<'static> {
	FontRef::try_from_slice(FONT).expect("embedded font")
}

fn text_width(font: &FontRef, px: f32, text: &str) -> f32 {
	let scaled = font.as_scaled(PxScale::from(px));
	let mut w = 0.0;
	let mut prev = None;
	for c in text.chars() {
		let id = font.glyph_id(c);
		if let Some(p) = prev {
			w += scaled.kern(p, id);
		}
		w += scaled.h_advance(id);
		prev = Some(id);
	}
	w
}

/// Largest size up to `max_px` at which `text` fits `width`; long text is shortened with an ellipsis.
fn fit(font: &FontRef, text: &str, max_px: f32, min_px: f32, width: f32) -> (f32, String) {
	let mut px = max_px;
	while px > min_px && text_width(font, px, text) > width {
		px -= 1.0;
	}
	if text_width(font, px, text) <= width {
		return (px, text.to_owned());
	}
	let mut s: String = text.to_owned();
	while !s.is_empty() && text_width(font, px, &format!("{}…", s.trim_end())) > width {
		s.pop();
	}
	(px, format!("{}…", s.trim_end()))
}

fn draw_text(canvas: &mut RgbaImage, font: &FontRef, px: f32, text: &str, baseline: f32, fg: Rgba<u8>) {
	let scale = PxScale::from(px);
	let scaled = font.as_scaled(scale);
	let mut x = (canvas.width() as f32 - text_width(font, px, text)) / 2.0;
	let mut prev = None;
	for c in text.chars() {
		let id = font.glyph_id(c);
		if let Some(p) = prev {
			x += scaled.kern(p, id);
		}
		let glyph = id.with_scale_and_position(scale, point(x, baseline));
		x += scaled.h_advance(id);
		prev = Some(id);
		let Some(outlined) = font.outline_glyph(glyph) else { continue };
		let b = outlined.px_bounds();
		outlined.draw(|gx, gy, cov| {
			let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
			if px >= 0 && py >= 0 && (px as u32) < canvas.width() && (py as u32) < canvas.height() {
				let dst = canvas.get_pixel_mut(px as u32, py as u32);
				for i in 0..3 {
					dst[i] = (fg[i] as f32 * cov + dst[i] as f32 * (1.0 - cov)).round() as u8;
				}
			}
		});
	}
}

fn fill(canvas: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, colour: Rgba<u8>) {
	for y in y0..y1.min(canvas.height()) {
		for x in x0..x1.min(canvas.width()) {
			canvas.put_pixel(x, y, colour);
		}
	}
}

pub fn temperature(t: f64) -> String {
	format!("{t:.1}°")
}

pub fn draw(key: &Key) -> RgbaImage {
	let font = font();
	let size = KEY_SIZE;
	let mut canvas = RgbaImage::from_pixel(size, size, if key.selected { BG_SELECTED } else { BG });
	if key.selected {
		let b = 7;
		fill(&mut canvas, 0, 0, size, b, ACCENT);
		fill(&mut canvas, 0, size - b, size, size, ACCENT);
		fill(&mut canvas, 0, 0, b, size, ACCENT);
		fill(&mut canvas, size - b, 0, size, size, ACCENT);
	}
	let width = size as f32 - 20.0;

	let (px, name) = fit(&font, key.name, 24.0, 15.0, width);
	draw_text(&mut canvas, &font, px, &name, 34.0, if key.selected { ACCENT } else { MUTED });

	let current = key.state.and_then(|s| s.current).map(temperature).unwrap_or_else(|| "--°".into());
	let (px, current) = fit(&font, &current, 46.0, 20.0, width);
	draw_text(&mut canvas, &font, px, &current, 88.0, WHITE);

	let (line, colour) = if let Some(notice) = key.notice {
		(notice.to_owned(), MUTED)
	} else if let Some(pending) = key.pending {
		(target_line(pending), PENDING)
	} else if let Some(state) = key.state {
		let colour = if state.target.is_none() { COLD } else if state.manual { ACCENT } else { WHITE };
		(target_line(state.target), colour)
	} else {
		(String::new(), WHITE)
	};
	let (px, line) = fit(&font, &line, 26.0, 13.0, width);
	draw_text(&mut canvas, &font, px, &line, 122.0, colour);

	// Heating demand bar along the bottom edge.
	if let Some(heating) = key.state.and_then(|s| s.heating).filter(|h| *h > 0.0) {
		let inset = if key.selected { 7 } else { 0 };
		let usable = size - 2 * inset;
		let w = ((usable as f64) * heating.clamp(0.0, 100.0) / 100.0).round().max(4.0) as u32;
		let bar = Rgba([255, 90, 40, 255]);
		fill(&mut canvas, inset, size - inset - 5, inset + w, size - inset, bar);
	}
	canvas
}

fn target_line(target: Option<f64>) -> String {
	match target {
		Some(t) => format!("→ {}", temperature(t)),
		None => "Off".into(),
	}
}

/// A plain notice key, e.g. before a room is chosen.
pub fn message(title: &str, line: &str) -> RgbaImage {
	let font = font();
	let mut canvas = RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, BG);
	let width = KEY_SIZE as f32 - 20.0;
	let (px, t) = fit(&font, title, 26.0, 14.0, width);
	draw_text(&mut canvas, &font, px, &t, 66.0, WHITE);
	let (px, l) = fit(&font, line, 18.0, 12.0, width);
	draw_text(&mut canvas, &font, px, &l, 98.0, MUTED);
	canvas
}

pub fn data_url(img: &RgbaImage) -> String {
	let mut png = Vec::new();
	img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).expect("png");
	format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

/// A QR code for the sign-in page, as SVG for the property inspector.
pub fn qr_svg(url: &str) -> Option<String> {
	let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
	Some(
		code.render::<qrcode::render::svg::Color>()
			.min_dimensions(160, 160)
			.dark_color(qrcode::render::svg::Color("#000000"))
			.light_color(qrcode::render::svg::Color("#ffffff"))
			.build(),
	)
}
