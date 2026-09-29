//! Text from DejaVu glyph atlases embedded in the binary (see
//! `tools/make_font_atlas.py` and `assets/syrup/fonts`).

use std::collections::HashMap;
use std::sync::OnceLock;

use image::GrayImage;
use serde::Deserialize;

use crate::{Color, Painter};

/// Which face and how big (pixels per em).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontStyle {
    pub size: f32,
    pub bold: bool,
}

impl FontStyle {
    pub fn regular(size: f32) -> Self {
        FontStyle { size, bold: false }
    }

    pub fn bold(size: f32) -> Self {
        FontStyle { size, bold: true }
    }

    /// Distance between baselines.
    pub fn line_height(&self) -> f32 {
        let (atlas, k) = pick(*self);
        atlas.line_height as f32 * k
    }

    pub fn ascent(&self) -> f32 {
        let (atlas, k) = pick(*self);
        atlas.ascent as f32 * k
    }
}

#[derive(Deserialize)]
struct AtlasMeta {
    size: u32,
    ascent: i32,
    #[allow(dead_code)]
    descent: i32,
    line_height: i32,
    glyphs: HashMap<String, [f32; 7]>,
}

struct Glyph {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    advance: f32,
}

struct Atlas {
    size: u32,
    bold: bool,
    ascent: i32,
    line_height: i32,
    glyphs: HashMap<char, Glyph>,
    alpha: GrayImage,
}

macro_rules! atlas_png {
    ($name:literal) => {
        ($name, include_bytes!(concat!("../../../assets/syrup/fonts/", $name, ".png")) as &[u8])
    };
}

const PNGS: &[(&str, &[u8])] = &[
    atlas_png!("sans-11"),
    atlas_png!("sans-13"),
    atlas_png!("sans-16"),
    atlas_png!("sans-20"),
    atlas_png!("sans-bold-11"),
    atlas_png!("sans-bold-13"),
    atlas_png!("sans-bold-16"),
    atlas_png!("sans-bold-20"),
    atlas_png!("sans-bold-26"),
    atlas_png!("sans-bold-34"),
    atlas_png!("sans-bold-48"),
    atlas_png!("sans-bold-72"),
];

const META: &str = include_str!("../../../assets/syrup/fonts/atlas.json");

fn atlases() -> &'static Vec<Atlas> {
    static ATLASES: OnceLock<Vec<Atlas>> = OnceLock::new();
    ATLASES.get_or_init(|| {
        let meta: HashMap<String, AtlasMeta> = serde_json::from_str(META).unwrap_or_default();
        let mut out = Vec::new();
        for (name, png) in PNGS {
            let Some(m) = meta.get(*name) else { continue };
            let Ok(img) = image::load_from_memory_with_format(png, image::ImageFormat::Png) else {
                continue;
            };
            let glyphs = m
                .glyphs
                .iter()
                .filter_map(|(k, v)| {
                    let ch = k.chars().next()?;
                    Some((
                        ch,
                        Glyph {
                            x: v[0] as u32,
                            y: v[1] as u32,
                            w: v[2] as u32,
                            h: v[3] as u32,
                            x0: v[4],
                            y0: v[5],
                            advance: v[6],
                        },
                    ))
                })
                .collect();
            out.push(Atlas {
                size: m.size,
                bold: name.contains("bold"),
                ascent: m.ascent,
                line_height: m.line_height,
                glyphs,
                alpha: img.to_luma8(),
            });
        }
        out.sort_by_key(|a| (a.bold, a.size));
        out
    })
}

/// The atlas to draw `style` from, and the scale to apply: the smallest
/// atlas at least as big (shrinking looks better than growing), the largest
/// otherwise.
fn pick(style: FontStyle) -> (&'static Atlas, f32) {
    let all = atlases();
    let same: Vec<&Atlas> = all.iter().filter(|a| a.bold == style.bold).collect();
    let pool = if same.is_empty() { all.iter().collect() } else { same };
    let size = style.size.max(4.0);
    let chosen =
        pool.iter().find(|a| a.size as f32 >= size - 0.5).or(pool.last()).copied().expect("font atlases are embedded");
    (chosen, size / chosen.size as f32)
}

fn glyph(atlas: &Atlas, ch: char) -> Option<&Glyph> {
    atlas.glyphs.get(&ch).or_else(|| match ch {
        '\u{a0}' => atlas.glyphs.get(&' '),
        _ => atlas.glyphs.get(&'?'),
    })
}

/// Width of `text` in pixels.
pub fn measure(text: &str, style: FontStyle) -> f32 {
    let (atlas, k) = pick(style);
    text.chars().filter_map(|c| glyph(atlas, c)).map(|g| g.advance * k).sum()
}

/// Splits `text` into lines no wider than `max_width` (words are never split
/// unless a single word is wider than the line).
pub fn wrap(text: &str, style: FontStyle, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if measure(&candidate, style) <= max_width || line.is_empty() {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        lines.push(line);
    }
    lines
}

impl Painter<'_> {
    /// Draws `text` with its top (the ascent line) at `y`; returns its width.
    pub fn text(&mut self, x: f32, y: f32, text: &str, style: FontStyle, color: Color) -> f32 {
        let (atlas, k) = pick(style);
        let baseline = y + atlas.ascent as f32 * k;
        let mut pen = x;
        for ch in text.chars() {
            let Some(g) = glyph(atlas, ch) else { continue };
            if g.w > 0 && g.h > 0 {
                self.glyph(atlas, g, pen + g.x0 * k, baseline + g.y0 * k, k, color);
            }
            pen += g.advance * k;
        }
        pen - x
    }

    /// Centred on `cx`.
    pub fn text_centered(&mut self, cx: f32, y: f32, text: &str, style: FontStyle, color: Color) -> f32 {
        let w = measure(text, style);
        self.text(cx - w / 2.0, y, text, style, color)
    }

    /// With an outline, for text over busy pictures.
    pub fn text_outlined(&mut self, x: f32, y: f32, text: &str, style: FontStyle, color: Color, outline: Color) -> f32 {
        let r = (style.size / 14.0).clamp(1.0, 3.0);
        for (dx, dy) in [
            (-r, 0.0),
            (r, 0.0),
            (0.0, -r),
            (0.0, r),
            (-r * 0.7, -r * 0.7),
            (r * 0.7, r * 0.7),
            (-r * 0.7, r * 0.7),
            (r * 0.7, -r * 0.7),
        ] {
            self.text(x + dx, y + dy, text, style, outline);
        }
        self.text(x, y, text, style, color)
    }

    fn glyph(&mut self, atlas: &Atlas, g: &Glyph, gx: f32, gy: f32, k: f32, color: Color) {
        let (dw, dh) = ((g.w as f32 * k).ceil() as i32 + 1, (g.h as f32 * k).ceil() as i32 + 1);
        let (ox, oy) = (gx.floor() as i32, gy.floor() as i32);
        let (fx, fy) = (gx - ox as f32, gy - oy as f32);
        for dy in 0..dh {
            for dx in 0..dw {
                // Position inside the glyph box, in atlas pixels.
                let u = (dx as f32 + 0.5 - fx) / k - 0.5;
                let v = (dy as f32 + 0.5 - fy) / k - 0.5;
                let a = if k < 0.75 {
                    sample_area(&atlas.alpha, g, u, v, 1.0 / k)
                } else {
                    sample_bilinear(&atlas.alpha, g, u, v)
                };
                if a > 0.0 {
                    self.blend(ox + dx, oy + dy, color, a);
                }
            }
        }
    }
}

fn texel(alpha: &GrayImage, g: &Glyph, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= g.w as i32 || y >= g.h as i32 {
        return 0.0;
    }
    alpha.get_pixel(g.x + x as u32, g.y + y as u32).0[0] as f32 / 255.0
}

fn sample_bilinear(alpha: &GrayImage, g: &Glyph, u: f32, v: f32) -> f32 {
    let (x0, y0) = (u.floor() as i32, v.floor() as i32);
    let (fx, fy) = (u - x0 as f32, v - y0 as f32);
    let a = texel(alpha, g, x0, y0) * (1.0 - fx) + texel(alpha, g, x0 + 1, y0) * fx;
    let b = texel(alpha, g, x0, y0 + 1) * (1.0 - fx) + texel(alpha, g, x0 + 1, y0 + 1) * fx;
    a * (1.0 - fy) + b * fy
}

fn sample_area(alpha: &GrayImage, g: &Glyph, u: f32, v: f32, span: f32) -> f32 {
    let (x0, y0) = ((u - span / 2.0 + 0.5).floor() as i32, (v - span / 2.0 + 0.5).floor() as i32);
    let n = span.ceil().max(1.0) as i32;
    let mut sum = 0.0;
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            sum += texel(alpha, g, x, y);
        }
    }
    sum / (n * n) as f32
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::*;
    use crate::rgb;

    #[test]
    fn every_atlas_loads_and_text_measures_sensibly() {
        assert_eq!(atlases().len(), PNGS.len());
        let w16 = measure("Health 87/100", FontStyle::bold(16.0));
        let w32 = measure("Health 87/100", FontStyle::bold(32.0));
        assert!(w16 > 60.0 && w16 < 160.0, "{w16}");
        assert!((w32 / w16 - 2.0).abs() < 0.15, "{w32} {w16}");
        let lines = wrap("Wait. That attack repeats every four seconds.", FontStyle::regular(16.0), 150.0);
        assert!(
            lines.len() >= 2 && lines.iter().all(|l| measure(l, FontStyle::regular(16.0)) <= 150.0 || !l.contains(' ')),
            "{lines:?}"
        );
    }

    #[test]
    fn text_is_drawn_where_asked() {
        let mut img = RgbaImage::new(200, 40);
        let w = Painter::new(&mut img).text(10.0, 5.0, "YOU DIED", FontStyle::bold(20.0), rgb(200, 0, 0));
        let inked: Vec<(u32, u32)> = img.enumerate_pixels().filter(|p| p.2.0[3] > 128).map(|p| (p.0, p.1)).collect();
        assert!(!inked.is_empty());
        let (min_x, max_x) = (inked.iter().map(|p| p.0).min().unwrap(), inked.iter().map(|p| p.0).max().unwrap());
        let (min_y, max_y) = (inked.iter().map(|p| p.1).min().unwrap(), inked.iter().map(|p| p.1).max().unwrap());
        assert!(min_x >= 10 && (max_x as f32) <= 10.0 + w + 1.0, "{min_x}..{max_x} w={w}");
        assert!(min_y >= 5 && max_y <= 5 + 22, "{min_y}..{max_y}");
    }
}
