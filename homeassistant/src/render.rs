//! Draws key images: an icon, a big value, a small line, the name, an optional level bar and a focus ring.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use base64::Engine;
use image::{DynamicImage, Rgba, RgbaImage};

use crate::layout::Nav;
use crate::ui::{Accent, KeyView, Tile};

pub const KEY_SIZE: u32 = 144;
static FONT: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold.ttf");

const BG: [u8; 3] = [24, 24, 27];

fn accent_rgb(a: Accent) -> [u8; 3] {
	match a {
		Accent::Neutral => [142, 142, 147],
		Accent::Room => [90, 160, 255],
		Accent::Heat => [255, 140, 50],
		Accent::Cool => [80, 190, 255],
		Accent::Light => [255, 205, 60],
		Accent::Cover => [160, 130, 255],
		Accent::Fan => [80, 210, 170],
		Accent::Media => [240, 90, 160],
		Accent::Switch => [120, 200, 90],
		Accent::Scene => [200, 160, 255],
		Accent::Warning => [255, 170, 40],
	}
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
	[0, 1, 2].map(|i| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t).round() as u8)
}

fn rgba(c: [u8; 3]) -> Rgba<u8> {
	Rgba([c[0], c[1], c[2], 255])
}

fn fill_rect(img: &mut RgbaImage, x0: u32, y0: u32, w: u32, h: u32, c: [u8; 3]) {
	for y in y0..(y0 + h).min(img.height()) {
		for x in x0..(x0 + w).min(img.width()) {
			img.put_pixel(x, y, rgba(c));
		}
	}
}

fn text_width(font: &FontRef, px: f32, text: &str) -> f32 {
	let scaled = font.as_scaled(PxScale::from(px));
	let mut w = 0.0;
	let mut prev = None;
	for c in text.chars() {
		let id = scaled.glyph_id(c);
		if let Some(p) = prev {
			w += scaled.kern(p, id);
		}
		w += scaled.h_advance(id);
		prev = Some(id);
	}
	w
}

/// Largest size up to `max_px` at which `text` fits `width`, and the text (ellipsised if even `min_px` is too big).
fn fit(font: &FontRef, text: &str, max_px: f32, min_px: f32, width: f32) -> (f32, String) {
	let mut px = max_px;
	while px > min_px && text_width(font, px, text) > width {
		px -= 1.0;
	}
	if text_width(font, px, text) <= width {
		return (px, text.to_owned());
	}
	let mut chars: Vec<char> = text.chars().collect();
	while !chars.is_empty() {
		chars.pop();
		let s: String = chars.iter().collect::<String>().trim_end().to_owned() + "…";
		if text_width(font, px, &s) <= width {
			return (px, s);
		}
	}
	(px, String::new())
}

/// Draws `text` centred horizontally on `cx` with its baseline at `baseline`.
fn draw_text(img: &mut RgbaImage, font: &FontRef, px: f32, text: &str, cx: f32, baseline: f32, colour: [u8; 3]) {
	let scaled = font.as_scaled(PxScale::from(px));
	let mut x = cx - text_width(font, px, text) / 2.0;
	let mut prev = None;
	for c in text.chars() {
		let id = scaled.glyph_id(c);
		if let Some(p) = prev {
			x += scaled.kern(p, id);
		}
		let glyph = id.with_scale_and_position(PxScale::from(px), point(x, baseline));
		x += scaled.h_advance(id);
		prev = Some(id);
		if let Some(outlined) = font.outline_glyph(glyph) {
			let b = outlined.px_bounds();
			outlined.draw(|gx, gy, cov| {
				let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
				if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
					let dst = img.get_pixel_mut(px as u32, py as u32);
					for i in 0..3 {
						dst[i] = (colour[i] as f32 * cov + dst[i] as f32 * (1.0 - cov)).round() as u8;
					}
				}
			});
		}
	}
}

/// Splits a name into at most two lines that fit `width` at `px`.
fn two_lines(font: &FontRef, text: &str, px: f32, width: f32) -> Vec<String> {
	if text_width(font, px, text) <= width {
		return vec![text.to_owned()];
	}
	let words: Vec<&str> = text.split_whitespace().collect();
	for split in (1..words.len()).rev() {
		let first = words[..split].join(" ");
		if text_width(font, px, &first) <= width {
			let rest = words[split..].join(" ");
			let (_, rest) = fit(font, &rest, px, px, width);
			return vec![first, rest];
		}
	}
	vec![fit(font, text, px, px, width).1]
}

pub fn tile(t: &Tile) -> RgbaImage {
	let font = FontRef::try_from_slice(FONT).expect("embedded font");
	let accent = accent_rgb(t.accent.unwrap_or(Accent::Neutral));
	let bg = if t.on { mix(BG, accent, 0.28) } else { BG };
	let mut img = RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, rgba(bg));
	let fg = if t.dim { [110, 110, 115] } else { [245, 245, 247] };
	let soft = if t.dim { [90, 90, 95] } else { [190, 190, 196] };
	let s = KEY_SIZE as f32;
	let margin = 8.0;
	let width = s - 2.0 * margin;

	// Icon, top left; big value to its right or centred when there is no value.
	let has_value = !t.value.is_empty();
	if !t.icon.is_empty() {
		let icon_colour = if t.on || !has_value { accent } else { mix(accent, soft, 0.4) };
		if has_value {
			draw_text(&mut img, &font, 26.0, &t.icon, margin + 14.0, 34.0, icon_colour);
		} else {
			draw_text(&mut img, &font, 54.0, &t.icon, s / 2.0, 72.0, icon_colour);
		}
	}
	if has_value {
		let (px, value) = fit(&font, &t.value, 44.0, 20.0, width);
		draw_text(&mut img, &font, px, &value, s / 2.0, 80.0, fg);
	}
	if !t.sub.is_empty() {
		let (px, sub) = fit(&font, &t.sub, 16.0, 12.0, width);
		draw_text(&mut img, &font, px, &sub, s / 2.0, if has_value { 100.0 } else { 94.0 }, soft);
	}

	// Name along the bottom, up to two lines.
	let lines = two_lines(&font, &t.title, 17.0, width);
	let (px, lines) = if lines.len() > 1 && !t.sub.is_empty() {
		let (px, line) = fit(&font, &t.title, 17.0, 12.0, width);
		(px, vec![line])
	} else {
		(17.0, lines)
	};
	let bottom = if t.level.is_some() { s - 16.0 } else { s - 10.0 };
	for (i, line) in lines.iter().enumerate() {
		let baseline = bottom - (lines.len() - 1 - i) as f32 * (px + 2.0);
		draw_text(&mut img, &font, px, line, s / 2.0, baseline, fg);
	}

	if let Some(level) = t.level {
		let w = (KEY_SIZE - 16) as f32;
		fill_rect(&mut img, 8, KEY_SIZE - 9, w as u32, 4, mix(bg, [255, 255, 255], 0.15));
		fill_rect(&mut img, 8, KEY_SIZE - 9, (w * level.clamp(0.0, 1.0)).round() as u32, 4, accent);
	}

	if t.focused {
		let c = [255, 255, 255];
		let b = 5;
		fill_rect(&mut img, 0, 0, KEY_SIZE, b, c);
		fill_rect(&mut img, 0, KEY_SIZE - b, KEY_SIZE, b, c);
		fill_rect(&mut img, 0, 0, b, KEY_SIZE, c);
		fill_rect(&mut img, KEY_SIZE - b, 0, b, KEY_SIZE, c);
	}
	img
}

pub fn key(view: &KeyView) -> RgbaImage {
	match view {
		KeyView::Blank => RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, Rgba([0, 0, 0, 255])),
		KeyView::Tile(t) => tile(t),
		KeyView::Nav(nav, enabled) => {
			let (icon, title) = match nav {
				Nav::Back => ("←", "Back"),
				Nav::Home => ("⌂", "Rooms"),
				Nav::Next => ("→", "More"),
			};
			tile(&Tile { icon: icon.into(), title: title.into(), accent: Some(Accent::Neutral), dim: !enabled, ..Default::default() })
		}
	}
}

pub fn to_png(img: &RgbaImage) -> Vec<u8> {
	let mut out = std::io::Cursor::new(Vec::new());
	DynamicImage::ImageRgba8(img.clone()).write_to(&mut out, image::ImageFormat::Png).expect("png encode");
	out.into_inner()
}

pub fn data_url(img: &RgbaImage) -> String {
	format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(to_png(img)))
}
