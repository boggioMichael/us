//! Pixel helpers: a downscaled working copy of the frame, luminance,
//! colour measures, perceptual hashes.

use image::RgbaImage;
use syrup_core::Rect;

#[inline]
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8
}

/// max - min of the channels: 0 for greys, high for vivid colours.
#[inline]
pub fn chroma(r: u8, g: u8, b: u8) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

/// Hue in degrees (0..360); meaningless for greys.
pub fn hue(r: u8, g: u8, b: u8) -> f32 {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d <= 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

/// Distance between two hues on the circle, degrees.
#[inline]
pub fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

pub fn color_distance(a: [u8; 3], b: [u8; 3]) -> f32 {
    let d = |i: usize| a[i] as f32 - b[i] as f32;
    (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
}

/// A small copy of the frame (area-averaged), for everything that does not
/// need full resolution: stability, motion, scene signatures.
#[derive(Debug, Clone)]
pub struct WorkImage {
    pub w: usize,
    pub h: usize,
    /// Frame pixels per work pixel.
    pub scale: f32,
    pub rgb: Vec<[u8; 3]>,
    pub luma: Vec<u8>,
}

impl WorkImage {
    pub fn from_frame(img: &RgbaImage, target_w: usize) -> Self {
        let (fw, fh) = (img.width() as usize, img.height() as usize);
        let w = target_w.min(fw).max(1);
        let scale = fw as f32 / w as f32;
        let h = ((fh as f32 / scale).round() as usize).max(1);
        let step = if scale > 4.0 { 2 } else { 1 };
        let raw = img.as_raw();
        let mut rgb = Vec::with_capacity(w * h);
        let mut luma_v = Vec::with_capacity(w * h);
        for y in 0..h {
            let y0 = ((y as f32 * scale) as usize).min(fh - 1);
            let y1 = (((y + 1) as f32 * scale) as usize).clamp(y0 + 1, fh);
            for x in 0..w {
                let x0 = ((x as f32 * scale) as usize).min(fw - 1);
                let x1 = (((x + 1) as f32 * scale) as usize).clamp(x0 + 1, fw);
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                let mut yy = y0;
                while yy < y1 {
                    let row = yy * fw * 4;
                    let mut xx = x0;
                    while xx < x1 {
                        let i = row + xx * 4;
                        r += raw[i] as u32;
                        g += raw[i + 1] as u32;
                        b += raw[i + 2] as u32;
                        n += 1;
                        xx += step;
                    }
                    yy += step;
                }
                let c = [(r / n) as u8, (g / n) as u8, (b / n) as u8];
                rgb.push(c);
                luma_v.push(luma(c[0], c[1], c[2]));
            }
        }
        WorkImage { w, h, scale, rgb, luma: luma_v }
    }

    /// A rectangle in work pixels, in frame pixels.
    pub fn to_frame(&self, x: usize, y: usize, w: usize, h: usize) -> Rect {
        let s = self.scale;
        Rect::new(
            (x as f32 * s).floor() as i32,
            (y as f32 * s).floor() as i32,
            (w as f32 * s).ceil() as u32,
            (h as f32 * s).ceil() as u32,
        )
    }

    /// A rectangle in frame pixels, in work pixels (clipped).
    pub fn from_frame_rect(&self, r: Rect) -> (usize, usize, usize, usize) {
        let s = self.scale;
        let x0 = ((r.x as f32 / s).floor().max(0.0) as usize).min(self.w);
        let y0 = ((r.y as f32 / s).floor().max(0.0) as usize).min(self.h);
        let x1 = (((r.x as f32 + r.w as f32) / s).ceil().max(0.0) as usize).min(self.w);
        let y1 = (((r.y as f32 + r.h as f32) / s).ceil().max(0.0) as usize).min(self.h);
        (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }
}

/// A `tw` x `th` luminance thumbnail, by sampling (cheap enough for every frame).
pub fn thumbnail(img: &RgbaImage, tw: usize, th: usize) -> Vec<u8> {
    let (fw, fh) = (img.width() as usize, img.height() as usize);
    let raw = img.as_raw();
    let mut out = Vec::with_capacity(tw * th);
    if fw == 0 || fh == 0 {
        return vec![0; tw * th];
    }
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = 0u32;
            for (ox, oy) in [(0.25f32, 0.25f32), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let x = (((tx as f32 + ox) / tw as f32) * fw as f32) as usize;
                let y = (((ty as f32 + oy) / th as f32) * fh as f32) as usize;
                let i = (y.min(fh - 1) * fw + x.min(fw - 1)) * 4;
                sum += luma(raw[i], raw[i + 1], raw[i + 2]) as u32;
            }
            out.push((sum / 4) as u8);
        }
    }
    out
}

/// Mean absolute difference of two thumbnails, 0..1.
pub fn thumb_difference(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs()).sum::<u32>() as f32 / (a.len() as f32 * 255.0)
}

/// Difference hash (9x8 area samples, 64 bits) of a region of the frame.
pub fn dhash(img: &RgbaImage, r: Rect) -> u64 {
    let Some(r) = r.clip(img.width(), img.height()) else {
        return 0;
    };
    if r.w < 2 || r.h < 2 {
        return 0;
    }
    let raw = img.as_raw();
    let fw = img.width() as usize;
    let mut cells = [[0f32; 9]; 8];
    for (cy, row) in cells.iter_mut().enumerate() {
        for (cx, cell) in row.iter_mut().enumerate() {
            let x0 = r.x as usize + (cx * r.w as usize) / 9;
            let x1 = (r.x as usize + ((cx + 1) * r.w as usize) / 9).max(x0 + 1);
            let y0 = r.y as usize + (cy * r.h as usize) / 8;
            let y1 = (r.y as usize + ((cy + 1) * r.h as usize) / 8).max(y0 + 1);
            let (mut s, mut n) = (0u32, 0u32);
            let sx = ((x1 - x0) / 4).max(1);
            let sy = ((y1 - y0) / 4).max(1);
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let i = (y * fw + x) * 4;
                    s += luma(raw[i], raw[i + 1], raw[i + 2]) as u32;
                    n += 1;
                    x += sx;
                }
                y += sy;
            }
            *cell = s as f32 / n.max(1) as f32;
        }
    }
    let mut h = 0u64;
    for (cy, row) in cells.iter().enumerate() {
        for cx in 0..8 {
            if row[cx] < row[cx + 1] {
                h |= 1 << (cy * 8 + cx);
            }
        }
    }
    h
}

/// Mean colour of a region of the frame (sampled).
pub fn mean_color(img: &RgbaImage, r: Rect) -> [u8; 3] {
    let Some(r) = r.clip(img.width(), img.height()) else {
        return [0, 0, 0];
    };
    if r.w == 0 || r.h == 0 {
        return [0, 0, 0];
    }
    let (mut s, mut n) = ([0u64; 3], 0u64);
    let sx = (r.w / 24).max(1);
    let sy = (r.h / 24).max(1);
    let mut y = r.y as u32;
    while y < r.y as u32 + r.h {
        let mut x = r.x as u32;
        while x < r.x as u32 + r.w {
            let p = img.get_pixel(x, y).0;
            for i in 0..3 {
                s[i] += p[i] as u64;
            }
            n += 1;
            x += sx;
        }
        y += sy;
    }
    [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_measures() {
        assert_eq!(chroma(10, 10, 10), 0);
        assert!((hue(255, 0, 0) - 0.0).abs() < 1e-3);
        assert!((hue(0, 255, 0) - 120.0).abs() < 1e-3);
        assert!((hue(0, 0, 255) - 240.0).abs() < 1e-3);
        assert!((hue_distance(350.0, 10.0) - 20.0).abs() < 1e-3);
    }

    #[test]
    fn work_image_maps_back_to_the_frame() {
        let img = RgbaImage::from_fn(960, 540, |x, _| {
            if x < 480 { image::Rgba([200, 0, 0, 255]) } else { image::Rgba([0, 0, 200, 255]) }
        });
        let w = WorkImage::from_frame(&img, 320);
        assert_eq!((w.w, w.h), (320, 180));
        assert_eq!(w.rgb[0], [200, 0, 0]);
        assert_eq!(w.rgb[319], [0, 0, 200]);
        let r = w.to_frame(10, 10, 20, 5);
        assert_eq!(r, Rect::new(30, 30, 60, 15));
        assert_eq!(w.from_frame_rect(r), (10, 10, 20, 5));
    }

    #[test]
    fn hashes_tell_pictures_apart() {
        let a = RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 0, 255]));
        let b = RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(255 - x * 4) as u8, (y * 4) as u8, 0, 255]));
        let full = Rect::new(0, 0, 64, 64);
        assert_eq!(dhash(&a, full), dhash(&a.clone(), full));
        assert!((dhash(&a, full) ^ dhash(&b, full)).count_ones() > 20);
    }
}
