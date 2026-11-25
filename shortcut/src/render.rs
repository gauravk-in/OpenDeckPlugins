//! Draws the key image: the shortcut on a keycap.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use base64::Engine;
use image::{Rgba, RgbaImage};

pub const SIZE: u32 = 144;
static FONT: &[u8] = include_bytes!("../fonts/DejaVuSans-Bold.ttf");

const BACKGROUND: Rgba<u8> = Rgba([18, 18, 20, 255]);
const CAP: Rgba<u8> = Rgba([52, 54, 60, 255]);
const CAP_EDGE: Rgba<u8> = Rgba([86, 90, 100, 255]);
const TEXT: Rgba<u8> = Rgba([245, 245, 245, 255]);

fn inside_rounded(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
	let cx = x.clamp(x0 + r, x1 - r);
	let cy = y.clamp(y0 + r, y1 - r);
	(x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn keycap() -> RgbaImage {
	let mut img = RgbaImage::from_pixel(SIZE, SIZE, BACKGROUND);
	let (outer, inner) = ((10.0, 12.0, 134.0, 136.0), (13.0, 14.0, 131.0, 128.0));
	for y in 0..SIZE {
		for x in 0..SIZE {
			let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
			if inside_rounded(fx, fy, inner.0, inner.1, inner.2, inner.3, 16.0) {
				img.put_pixel(x, y, CAP);
			} else if inside_rounded(fx, fy, outer.0, outer.1, outer.2, outer.3, 18.0) {
				img.put_pixel(x, y, CAP_EDGE);
			}
		}
	}
	img
}

fn text_width(font: &FontRef, scale: PxScale, text: &str) -> f32 {
	let f = font.as_scaled(scale);
	let mut w = 0.0;
	let mut prev = None;
	for c in text.chars() {
		let id = f.glyph_id(c);
		if let Some(p) = prev {
			w += f.kern(p, id);
		}
		w += f.h_advance(id);
		prev = Some(id);
	}
	w
}

fn draw_line(img: &mut RgbaImage, font: &FontRef, scale: PxScale, text: &str, centre_y: f32) {
	let f = font.as_scaled(scale);
	let width = text_width(font, scale, text);
	let mut x = (SIZE as f32 - width) / 2.0;
	let baseline = centre_y + (f.ascent() + f.descent()) / 2.0;
	let mut prev = None;
	for c in text.chars() {
		let id = f.glyph_id(c);
		if let Some(p) = prev {
			x += f.kern(p, id);
		}
		let glyph = id.with_scale_and_position(scale, point(x, baseline));
		x += f.h_advance(id);
		prev = Some(id);
		if let Some(outline) = font.outline_glyph(glyph) {
			let bounds = outline.px_bounds();
			outline.draw(|gx, gy, cov| {
				let (px, py) = (bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32);
				if px < 0 || py < 0 || px >= SIZE as i32 || py >= SIZE as i32 || cov <= 0.0 {
					return;
				}
				let dst = img.get_pixel_mut(px as u32, py as u32);
				for i in 0..3 {
					dst[i] = (TEXT[i] as f32 * cov + dst[i] as f32 * (1.0 - cov)).round() as u8;
				}
			});
		}
	}
}

/// Splits the label into at most three lines that fit, preferring breaks between combos, then after a "+".
fn lines(label: &str) -> Vec<String> {
	let words: Vec<&str> = label.split(' ').collect();
	if words.len() > 1 {
		return words.into_iter().take(3).map(str::to_owned).collect();
	}
	// One long Linux-style combo such as "Ctrl+Shift+Alt+T": break after a "+" near the middle.
	let chars: Vec<char> = label.chars().collect();
	if chars.len() > 9 && label.contains('+') {
		let mid = chars.len() / 2;
		let split = (0..chars.len()).filter(|&i| chars[i] == '+').min_by_key(|&i| i.abs_diff(mid)).unwrap_or(mid);
		return vec![chars[..=split].iter().collect(), chars[split + 1..].iter().collect()];
	}
	vec![label.to_owned()]
}

pub fn shortcut_image(label: &str) -> RgbaImage {
	lines_image(&lines(label))
}

/// Draws the given lines (at most three), as large as fits.
pub fn lines_image(lines: &[String]) -> RgbaImage {
	let mut img = keycap();
	let font = FontRef::try_from_slice(FONT).expect("bundled font");
	let lines = &lines[..lines.len().min(3)];
	let max_width = 104.0;
	let max_height = 96.0 / lines.len().max(1) as f32;
	let widest = lines.iter().map(|l| text_width(&font, PxScale::from(100.0), l)).fold(0.0, f32::max);
	let size = (100.0 * max_width / widest.max(1.0)).min(max_height * 0.8).min(56.0);
	let scale = PxScale::from(size);
	let line_height = size * 1.2;
	let top = 70.0 - line_height * (lines.len() as f32 - 1.0) / 2.0;
	for (i, line) in lines.iter().enumerate() {
		draw_line(&mut img, &font, scale, line, top + line_height * i as f32);
	}
	img
}

pub fn to_png(img: &RgbaImage) -> Vec<u8> {
	let mut out = std::io::Cursor::new(Vec::new());
	img.write_to(&mut out, image::ImageFormat::Png).expect("PNG encoding");
	out.into_inner()
}

pub fn data_url(png: &[u8]) -> String {
	format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}
