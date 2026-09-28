//! A small software painter for everything Syrup draws itself: the overlay
//! card, the mascot's layers, annotated frames for the devtools page, and the
//! synthetic test games.
//!
//! It paints into an [`RgbaImage`] (straight alpha), with antialiased shapes
//! and text rasterised from DejaVu atlases embedded in the binary, so the
//! result is identical on every platform and in every test.

mod font;
mod shapes;

pub use font::{FontStyle, measure, wrap};
use image::RgbaImage;
pub use shapes::lerp_color;

pub type Color = [u8; 4];

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r, g, b, 255]
}

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    [r, g, b, a]
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    [
        c[0],
        c[1],
        c[2],
        (c[3] as f32 * a.clamp(0.0, 1.0)).round() as u8,
    ]
}

/// Paints into an image, clipped to a rectangle.
pub struct Painter<'a> {
    pub img: &'a mut RgbaImage,
    clip: (i32, i32, i32, i32),
}

impl<'a> Painter<'a> {
    pub fn new(img: &'a mut RgbaImage) -> Self {
        let (w, h) = img.dimensions();
        Painter {
            img,
            clip: (0, 0, w as i32, h as i32),
        }
    }

    pub fn width(&self) -> u32 {
        self.img.width()
    }

    pub fn height(&self) -> u32 {
        self.img.height()
    }

    /// Restricts painting to `(x, y, w, h)` (within the image).
    pub fn set_clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (iw, ih) = (self.img.width() as i32, self.img.height() as i32);
        self.clip = (x.max(0), y.max(0), (x + w).min(iw), (y + h).min(ih));
    }

    pub fn reset_clip(&mut self) {
        self.clip = (0, 0, self.img.width() as i32, self.img.height() as i32);
    }

    /// Blends `c` over the pixel at `(x, y)` with extra coverage `cov` (0..1).
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Color, cov: f32) {
        let (x0, y0, x1, y1) = self.clip;
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return;
        }
        let a = (c[3] as f32 / 255.0) * cov.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let p = self.img.get_pixel_mut(x as u32, y as u32);
        blend_pixel(&mut p.0, c, a);
    }

    /// Sets the pixel, ignoring what was there.
    pub fn set(&mut self, x: i32, y: i32, c: Color) {
        let (x0, y0, x1, y1) = self.clip;
        if x >= x0 && y >= y0 && x < x1 && y < y1 {
            self.img.put_pixel(x as u32, y as u32, image::Rgba(c));
        }
    }

    pub fn clear(&mut self, c: Color) {
        for p in self.img.pixels_mut() {
            p.0 = c;
        }
    }

    /// Draws `src` with its top left at `(x, y)`, scaled to `w` x `h`, at `opacity`.
    pub fn image(&mut self, src: &RgbaImage, x: i32, y: i32, w: u32, h: u32, opacity: f32) {
        if w == 0 || h == 0 || src.width() == 0 || src.height() == 0 {
            return;
        }
        let (sw, sh) = (src.width() as f32, src.height() as f32);
        let sx = sw / w as f32;
        let sy = sh / h as f32;
        for dy in 0..h as i32 {
            let py = y + dy;
            if py < self.clip.1 || py >= self.clip.3 {
                continue;
            }
            for dx in 0..w as i32 {
                let px = x + dx;
                if px < self.clip.0 || px >= self.clip.2 {
                    continue;
                }
                let c = if sx > 1.5 || sy > 1.5 {
                    area_sample(src, dx as f32 * sx, dy as f32 * sy, sx, sy)
                } else {
                    bilinear(
                        src,
                        (dx as f32 + 0.5) * sx - 0.5,
                        (dy as f32 + 0.5) * sy - 0.5,
                    )
                };
                self.blend(px, py, c, opacity);
            }
        }
    }
}

/// Source-over with straight alpha.
#[inline]
pub fn blend_pixel(dst: &mut [u8; 4], c: Color, a: f32) {
    let da = dst[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    if out_a <= 0.0 {
        *dst = [0, 0, 0, 0];
        return;
    }
    for i in 0..3 {
        let v = (c[i] as f32 * a + dst[i] as f32 * da * (1.0 - a)) / out_a;
        dst[i] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round() as u8;
}

pub fn bilinear(src: &RgbaImage, x: f32, y: f32) -> Color {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |xx: i32, yy: i32| src.get_pixel(xx as u32, yy as u32).0;
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    // Interpolate premultiplied, so transparent pixels do not darken edges.
    let mut acc = [0f32; 4];
    for (px, wgt) in [
        (a, (1.0 - fx) * (1.0 - fy)),
        (b, fx * (1.0 - fy)),
        (c, (1.0 - fx) * fy),
        (d, fx * fy),
    ] {
        let al = px[3] as f32 / 255.0;
        for i in 0..3 {
            acc[i] += px[i] as f32 * al * wgt;
        }
        acc[3] += al * wgt;
    }
    unpremultiply(acc)
}

fn area_sample(src: &RgbaImage, x: f32, y: f32, sx: f32, sy: f32) -> Color {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (x1, y1) = (
        ((x + sx).ceil() as i32).min(w),
        ((y + sy).ceil() as i32).min(h),
    );
    let mut acc = [0f32; 4];
    let mut n = 0.0;
    for yy in y0.max(0)..y1.max(y0 + 1).min(h) {
        for xx in x0.max(0)..x1.max(x0 + 1).min(w) {
            let px = src.get_pixel(xx as u32, yy as u32).0;
            let al = px[3] as f32 / 255.0;
            for i in 0..3 {
                acc[i] += px[i] as f32 * al;
            }
            acc[3] += al;
            n += 1.0;
        }
    }
    if n == 0.0 {
        return [0, 0, 0, 0];
    }
    for v in acc.iter_mut() {
        *v /= n;
    }
    unpremultiply(acc)
}

fn unpremultiply(acc: [f32; 4]) -> Color {
    if acc[3] <= 1e-4 {
        return [0, 0, 0, 0];
    }
    [
        (acc[0] / acc[3]).round().clamp(0.0, 255.0) as u8,
        (acc[1] / acc[3]).round().clamp(0.0, 255.0) as u8,
        (acc[2] / acc[3]).round().clamp(0.0, 255.0) as u8,
        (acc[3] * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// A resized copy (area averaging when shrinking, bilinear when growing).
pub fn resize(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w.max(1), h.max(1));
    Painter::new(&mut out).image(src, 0, 0, w.max(1), h.max(1), 1.0);
    out
}

/// Decodes an embedded PNG.
pub fn load_png(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map(|i| i.to_rgba8())
        .unwrap_or_else(|_| RgbaImage::new(1, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending_and_resizing() {
        let mut img = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]));
        let mut p = Painter::new(&mut img);
        p.blend(1, 1, rgb(255, 255, 255), 0.5);
        p.blend(10, 10, rgb(255, 255, 255), 1.0); // outside: ignored
        assert_eq!(img.get_pixel(1, 1).0, [128, 128, 128, 255]);
        let big = resize(&img, 8, 8);
        assert_eq!(big.dimensions(), (8, 8));
        let small = resize(
            &RgbaImage::from_pixel(10, 10, image::Rgba([200, 100, 0, 255])),
            3,
            3,
        );
        assert_eq!(small.get_pixel(1, 1).0, [200, 100, 0, 255]);
    }

    #[test]
    fn transparent_edges_do_not_darken() {
        let mut src = RgbaImage::new(2, 1);
        src.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        src.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        let c = bilinear(&src, 0.5, 0.0);
        assert_eq!(&c[..3], &[255, 0, 0]);
        assert!((c[3] as i32 - 128).abs() <= 1);
    }
}
