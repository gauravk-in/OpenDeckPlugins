//! Turns a downloaded logo into a square key image, or draws a letter tile as a fallback.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use base64::Engine;
use image::{DynamicImage, Rgba, RgbaImage, imageops};

pub const KEY_SIZE: u32 = 144;
static FONT: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold.ttf");

pub fn parse_colour(s: &str) -> Option<Rgba<u8>> {
	let hex = s.trim().strip_prefix('#')?;
	let v = u32::from_str_radix(hex, 16).ok()?;
	match hex.len() {
		6 => Some(Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])),
		3 => {
			let (r, g, b) = (((v >> 8) & 0xf) as u8, ((v >> 4) & 0xf) as u8, (v & 0xf) as u8);
			Some(Rgba([r * 17, g * 17, b * 17, 255]))
		}
		_ => None,
	}
}

fn luminance(p: &Rgba<u8>) -> f32 {
	(0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0
}

/// Crops away fully transparent borders so small logos with lots of padding fill the key.
fn trim_transparent(img: &RgbaImage) -> RgbaImage {
	let (w, h) = img.dimensions();
	let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
	for (x, y, p) in img.enumerate_pixels() {
		if p[3] > 16 {
			x0 = x0.min(x);
			y0 = y0.min(y);
			x1 = x1.max(x);
			y1 = y1.max(y);
		}
	}
	if x1 < x0 || y1 < y0 {
		return img.clone();
	}
	imageops::crop_imm(img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

/// True when the logo is an opaque, square tile (typical apple-touch-icon) that should fill the key.
fn is_full_tile(img: &RgbaImage) -> bool {
	let (w, h) = img.dimensions();
	let ratio = w as f32 / h as f32;
	if !(0.9..=1.1).contains(&ratio) {
		return false;
	}
	let i = (w.min(h) / 20).max(1);
	[(i, i), (w - 1 - i, i), (i, h - 1 - i), (w - 1 - i, h - 1 - i)]
		.iter()
		.all(|&(x, y)| img.get_pixel(x, y)[3] > 200)
}

/// Picks a background: explicit colour, or (auto) white behind dark transparent logos, black otherwise.
fn pick_background(setting: &str, logo: &RgbaImage) -> Rgba<u8> {
	match setting.trim().to_ascii_lowercase().as_str() {
		"white" => Rgba([255, 255, 255, 255]),
		"black" | "" | "auto" => {
			if setting.trim().eq_ignore_ascii_case("black") {
				return Rgba([0, 0, 0, 255]);
			}
			let (mut sum, mut n) = (0.0f32, 0u32);
			for p in logo.pixels().filter(|p| p[3] > 128) {
				sum += luminance(p);
				n += 1;
			}
			let avg = if n == 0 { 1.0 } else { sum / n as f32 };
			if avg < 0.28 { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 255]) }
		}
		other => parse_colour(other).unwrap_or(Rgba([0, 0, 0, 255])),
	}
}

/// Composes the final key image. `padding` is the fraction of the key left empty on each side
/// for logos that do not fill the whole tile.
pub fn compose(logo: &DynamicImage, background: &str, padding: f32) -> RgbaImage {
	let rgba = logo.to_rgba8();
	if is_full_tile(&rgba) {
		return imageops::resize(&rgba, KEY_SIZE, KEY_SIZE, imageops::FilterType::Lanczos3);
	}
	let trimmed = trim_transparent(&rgba);
	let bg = pick_background(background, &trimmed);
	let mut canvas = RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, bg);

	let box_size = (KEY_SIZE as f32 * (1.0 - 2.0 * padding.clamp(0.0, 0.4))).round() as u32;
	let (w, h) = trimmed.dimensions();
	let scale = box_size as f32 / w.max(h) as f32;
	let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
	let filter = if scale > 1.0 && w.max(h) <= 32 { imageops::FilterType::CatmullRom } else { imageops::FilterType::Lanczos3 };
	let resized = imageops::resize(&trimmed, nw, nh, filter);
	imageops::overlay(&mut canvas, &resized, ((KEY_SIZE - nw) / 2) as i64, ((KEY_SIZE - nh) / 2) as i64);
	canvas
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

/// Draws one line of text horizontally centred on the canvas, with its baseline at `baseline`.
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

/// Splits a caption into lines that fit, choosing the largest font size that works.
/// Returns (font size, lines).
fn layout_caption(font: &FontRef, caption: &str, max_width: f32) -> (f32, Vec<String>) {
	for px in [26.0, 24.0, 22.0, 20.0, 18.0, 17.0, 16.0] {
		if text_width(font, px, caption) <= max_width {
			return (px, vec![caption.to_owned()]);
		}
	}
	// Two lines: break at the space that balances the lines best.
	let words: Vec<&str> = caption.split_whitespace().collect();
	if words.len() > 1 {
		for px in [20.0, 18.0, 17.0, 16.0, 15.0, 14.0] {
			let best = (1..words.len())
				.map(|i| (words[..i].join(" "), words[i..].join(" ")))
				.filter(|(a, b)| text_width(font, px, a) <= max_width && text_width(font, px, b) <= max_width)
				.min_by_key(|(a, b)| (text_width(font, px, a) - text_width(font, px, b)).abs() as i32);
			if let Some((a, b)) = best {
				return (px, vec![a, b]);
			}
		}
	}
	// Still too long: shorten with an ellipsis.
	let px = 16.0;
	let mut s: String = caption.to_owned();
	while !s.is_empty() && text_width(font, px, &format!("{s}…")) > max_width {
		s.pop();
	}
	(px, vec![format!("{}…", s.trim_end())])
}

/// Average colour of the outermost pixels, used to extend a tile's background behind the caption.
fn edge_colour(img: &RgbaImage) -> Rgba<u8> {
	let (w, h) = img.dimensions();
	let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
	for (x, y, p) in img.enumerate_pixels() {
		if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
			r += p[0] as u64;
			g += p[1] as u64;
			b += p[2] as u64;
			n += 1;
		}
	}
	let n = n.max(1);
	Rgba([(r / n) as u8, (g / n) as u8, (b / n) as u8, 255])
}

/// Shrinks a finished key image towards the top and writes `caption` underneath.
/// `background` is the user's setting; an explicit colour wins, otherwise the tile's own edge colour is used.
pub fn add_caption(key: &RgbaImage, caption: &str, background: &str) -> RgbaImage {
	let caption = caption.trim();
	if caption.is_empty() {
		return key.clone();
	}
	let font = FontRef::try_from_slice(FONT).expect("embedded font");
	let bg = match background.trim().to_ascii_lowercase().as_str() {
		"white" => Rgba([255, 255, 255, 255]),
		"black" => Rgba([0, 0, 0, 255]),
		other => parse_colour(other).unwrap_or_else(|| edge_colour(key)),
	};
	let fg = if luminance(&bg) > 0.6 { Rgba([20, 20, 20, 255]) } else { Rgba([255, 255, 255, 255]) };

	let margin = 6.0;
	let (px, lines) = layout_caption(&font, caption, KEY_SIZE as f32 - 2.0 * margin);
	let line_height = (px * 1.15).round();
	let text_block = line_height * lines.len() as f32;
	let logo_size = (KEY_SIZE as f32 - text_block - 3.0 * margin).clamp(56.0, 100.0) as u32;

	let mut canvas = RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, bg);
	let logo = imageops::resize(key, logo_size, logo_size, imageops::FilterType::Lanczos3);
	let top = margin as u32;
	imageops::overlay(&mut canvas, &logo, ((KEY_SIZE - logo_size) / 2) as i64, top as i64);

	let scaled = font.as_scaled(PxScale::from(px));
	let text_top = top as f32 + logo_size as f32 + (KEY_SIZE as f32 - top as f32 - logo_size as f32 - text_block) / 2.0;
	for (i, line) in lines.iter().enumerate() {
		let baseline = text_top + i as f32 * line_height + (line_height + scaled.ascent() + scaled.descent()) / 2.0;
		draw_text(&mut canvas, &font, px, line, baseline, fg);
	}
	canvas
}

/// Fallback: a coloured tile with the first letter of the site name.
pub fn letter_tile(label: &str, background: &str) -> RgbaImage {
	let letter = label.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_uppercase().next().unwrap_or('?');
	let bg = parse_colour(background).unwrap_or_else(|| {
		// Stable pleasant colour derived from the label.
		let hash = sha1_smol::Sha1::from(label.as_bytes()).digest().bytes();
		let hue = (u16::from_be_bytes([hash[0], hash[1]]) % 360) as f32;
		hsl_to_rgb(hue, 0.55, 0.45)
	});
	let mut canvas = RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, bg);
	let fg = if luminance(&bg) > 0.6 { Rgba([20, 20, 20, 255]) } else { Rgba([255, 255, 255, 255]) };

	let font = FontRef::try_from_slice(FONT).expect("embedded font");
	let scale = PxScale::from(KEY_SIZE as f32 * 0.62);
	let scaled = font.as_scaled(scale);
	let glyph = scaled.scaled_glyph(letter);
	if let Some(outlined) = font.outline_glyph(glyph.clone()) {
		let bounds = outlined.px_bounds();
		let dx = (KEY_SIZE as f32 - bounds.width()) / 2.0 - bounds.min.x;
		let dy = (KEY_SIZE as f32 - bounds.height()) / 2.0 - bounds.min.y;
		let mut g = glyph;
		g.position = point(dx, dy);
		if let Some(outlined) = font.outline_glyph(g) {
			let b = outlined.px_bounds();
			outlined.draw(|x, y, cov| {
				let (px, py) = (b.min.x as i32 + x as i32, b.min.y as i32 + y as i32);
				if px >= 0 && py >= 0 && (px as u32) < KEY_SIZE && (py as u32) < KEY_SIZE {
					let dst = canvas.get_pixel_mut(px as u32, py as u32);
					for i in 0..3 {
						dst[i] = (fg[i] as f32 * cov + dst[i] as f32 * (1.0 - cov)).round() as u8;
					}
				}
			});
		}
	}
	canvas
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgba<u8> {
	let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
	let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
	let m = l - c / 2.0;
	let (r, g, b) = match h as u32 {
		0..60 => (c, x, 0.0),
		60..120 => (x, c, 0.0),
		120..180 => (0.0, c, x),
		180..240 => (0.0, x, c),
		240..300 => (x, 0.0, c),
		_ => (c, 0.0, x),
	};
	let f = |v: f32| ((v + m) * 255.0).round() as u8;
	Rgba([f(r), f(g), f(b), 255])
}

pub fn to_png(img: &RgbaImage) -> Vec<u8> {
	let mut out = std::io::Cursor::new(Vec::new());
	DynamicImage::ImageRgba8(img.clone()).write_to(&mut out, image::ImageFormat::Png).expect("png encode");
	out.into_inner()
}

pub fn data_url(png: &[u8]) -> String {
	format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn letter_tile_draws_something() {
		let t = letter_tile("github.com", "");
		let corner = *t.get_pixel(2, 2);
		assert_eq!(t.dimensions(), (KEY_SIZE, KEY_SIZE));
		let drawn = t.pixels().filter(|p| **p != corner).count();
		assert!(drawn > 500, "letter should be drawn ({drawn} px)");
	}

	#[test]
	fn dark_transparent_logo_gets_white_background() {
		let mut img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
		for x in 16..48 {
			for y in 16..48 {
				img.put_pixel(x, y, Rgba([10, 10, 10, 255]));
			}
		}
		let out = compose(&DynamicImage::ImageRgba8(img), "auto", 0.12);
		assert_eq!(*out.get_pixel(1, 1), Rgba([255, 255, 255, 255]));
	}

	#[test]
	fn caption_shrinks_logo_and_draws_text() {
		let tile = RgbaImage::from_pixel(KEY_SIZE, KEY_SIZE, Rgba([30, 90, 200, 255]));
		let out = add_caption(&tile, "Pull requests", "auto");
		assert_eq!(out.dimensions(), (KEY_SIZE, KEY_SIZE));
		// Background extends the tile colour, and white text appears in the lower part.
		assert_eq!(*out.get_pixel(1, KEY_SIZE - 2), Rgba([30, 90, 200, 255]));
		let white = (110..KEY_SIZE).flat_map(|y| (0..KEY_SIZE).map(move |x| (x, y))).filter(|&(x, y)| out.get_pixel(x, y)[0] > 200).count();
		assert!(white > 100, "caption text expected ({white})");
		assert_eq!(add_caption(&tile, "  ", "auto"), tile);
	}

	#[test]
	fn long_captions_wrap_or_truncate() {
		let font = FontRef::try_from_slice(FONT).unwrap();
		let (_, lines) = layout_caption(&font, "Home Assistant energy dashboard", 132.0);
		assert_eq!(lines.len(), 2);
		let (_, lines) = layout_caption(&font, "Supercalifragilisticexpialidocious", 132.0);
		assert!(lines[0].ends_with('…'));
	}

	#[test]
	fn colours() {
		assert_eq!(parse_colour("#ff0000"), Some(Rgba([255, 0, 0, 255])));
		assert_eq!(parse_colour("#fff"), Some(Rgba([255, 255, 255, 255])));
		assert_eq!(parse_colour("nope"), None);
	}
}
