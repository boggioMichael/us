//! Antialiased shapes: rectangles, rounded rectangles, circles, ellipses,
//! thick lines and polygons. Coverage comes from signed distances (round
//! shapes) or exact span coverage on sub-scanlines (polygons).

use crate::{Color, Painter};

pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mut out = [0u8; 4];
    for i in 0..4 {
        out[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    }
    out
}

fn sdf_coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Signed distance from `(px, py)` to a rounded rectangle.
fn rounded_rect_sd(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let qx = (px - cx).abs() - (w / 2.0 - r);
    let qy = (py - cy).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

impl Painter<'_> {
    /// Whole-pixel rectangle.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, c, 1.0);
            }
        }
    }

    /// Rectangle outline, `t` pixels thick, inside the box.
    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, c: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        let t = t.max(1).min(w / 2 + 1).min(h / 2 + 1);
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h - t, w, t, c);
        self.fill_rect(x, y + t, t, h - 2 * t, c);
        self.fill_rect(x + w - t, y + t, t, h - 2 * t, c);
    }

    /// Top-to-bottom gradient.
    pub fn gradient_rect(&mut self, x: i32, y: i32, w: i32, h: i32, top: Color, bottom: Color) {
        for dy in 0..h.max(0) {
            let c = lerp_color(top, bottom, if h > 1 { dy as f32 / (h - 1) as f32 } else { 0.0 });
            for xx in x..x + w {
                self.blend(xx, y + dy, c, 1.0);
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        let (x0, y0) = ((x - 1.0).floor() as i32, (y - 1.0).floor() as i32);
        let (x1, y1) = ((x + w + 1.0).ceil() as i32, (y + h + 1.0).ceil() as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                let d = rounded_rect_sd(px as f32 + 0.5, py as f32 + 0.5, x, y, w, h, r);
                let cov = sdf_coverage(d);
                if cov > 0.0 {
                    self.blend(px, py, c, cov);
                }
            }
        }
    }

    /// A rounded rectangle's outline, `t` thick, centred on the box's edge inset by t/2.
    pub fn stroke_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, c: Color) {
        let half = t / 2.0;
        let (x0, y0) = ((x - 1.0).floor() as i32, (y - 1.0).floor() as i32);
        let (x1, y1) = ((x + w + 1.0).ceil() as i32, (y + h + 1.0).ceil() as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                let d = rounded_rect_sd(
                    px as f32 + 0.5,
                    py as f32 + 0.5,
                    x + half,
                    y + half,
                    w - t,
                    h - t,
                    (r - half).max(0.0),
                );
                let cov = sdf_coverage(d.abs() - half);
                if cov > 0.0 {
                    self.blend(px, py, c, cov);
                }
            }
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        self.fill_ellipse(cx, cy, r, r, c);
    }

    pub fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Color) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let (x0, y0) = ((cx - rx - 1.0).floor() as i32, (cy - ry - 1.0).floor() as i32);
        let (x1, y1) = ((cx + rx + 1.0).ceil() as i32, (cy + ry + 1.0).ceil() as i32);
        let k = rx.min(ry);
        for py in y0..y1 {
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx) / rx;
                let dy = (py as f32 + 0.5 - cy) / ry;
                // Approximate distance for an ellipse, exact for a circle.
                let d = ((dx * dx + dy * dy).sqrt() - 1.0) * k;
                let cov = sdf_coverage(d);
                if cov > 0.0 {
                    self.blend(px, py, c, cov);
                }
            }
        }
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, r: f32, t: f32, c: Color) {
        let (x0, y0) = ((cx - r - t).floor() as i32, (cy - r - t).floor() as i32);
        let (x1, y1) = ((cx + r + t).ceil() as i32, (cy + r + t).ceil() as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                let d = ((px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - r).abs() - t / 2.0;
                let cov = sdf_coverage(d);
                if cov > 0.0 {
                    self.blend(px, py, c, cov);
                }
            }
        }
    }

    /// A line `t` thick with round ends.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: Color) {
        let r = t / 2.0;
        let (bx0, by0) = ((x0.min(x1) - r - 1.0).floor() as i32, (y0.min(y1) - r - 1.0).floor() as i32);
        let (bx1, by1) = ((x0.max(x1) + r + 1.0).ceil() as i32, (y0.max(y1) + r + 1.0).ceil() as i32);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for py in by0..by1 {
            for px in bx0..bx1 {
                let (qx, qy) = (px as f32 + 0.5 - x0, py as f32 + 0.5 - y0);
                let h = ((qx * dx + qy * dy) / len2).clamp(0.0, 1.0);
                let d = (qx - dx * h).hypot(qy - dy * h) - r;
                let cov = sdf_coverage(d);
                if cov > 0.0 {
                    self.blend(px, py, c, cov);
                }
            }
        }
    }

    /// A filled polygon (even-odd), antialiased: 4 sub-scanlines per row with
    /// exact horizontal coverage.
    pub fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Color) {
        if pts.len() < 3 {
            return;
        }
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
        let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
        let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
        let width = (max_x - min_x + 2).max(1) as usize;
        const SUB: usize = 4;
        let mut cov = vec![0f32; width];
        let mut xs: Vec<f32> = Vec::new();
        for py in min_y..max_y {
            cov.iter_mut().for_each(|v| *v = 0.0);
            for s in 0..SUB {
                let sy = py as f32 + (s as f32 + 0.5) / SUB as f32;
                xs.clear();
                for i in 0..pts.len() {
                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                    if (a.1 <= sy && b.1 > sy) || (b.1 <= sy && a.1 > sy) {
                        xs.push(a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                for pair in xs.chunks_exact(2) {
                    let (l, r) = (pair[0] - min_x as f32, pair[1] - min_x as f32);
                    let (li, ri) = (l.floor().max(0.0) as usize, (r.floor() as usize).min(width - 1));
                    for (i, v) in cov.iter_mut().enumerate().take(ri + 1).skip(li) {
                        let lo = (i as f32).max(l);
                        let hi = (i as f32 + 1.0).min(r);
                        if hi > lo {
                            *v += (hi - lo) / SUB as f32;
                        }
                    }
                }
            }
            for (i, v) in cov.iter().enumerate() {
                if *v > 0.0 {
                    self.blend(min_x + i as i32, py, c, *v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::{Painter, rgb};

    fn coverage(img: &RgbaImage) -> f32 {
        img.pixels().map(|p| p.0[3] as f32 / 255.0).sum()
    }

    #[test]
    fn shapes_cover_their_area() {
        let mut img = RgbaImage::new(64, 64);
        Painter::new(&mut img).fill_circle(32.0, 32.0, 20.0, rgb(255, 0, 0));
        let area = coverage(&img);
        assert!((area - std::f32::consts::PI * 400.0).abs() < 15.0, "{area}");

        let mut img = RgbaImage::new(64, 64);
        Painter::new(&mut img).fill_polygon(&[(10.0, 10.0), (50.0, 10.0), (10.0, 50.0)], rgb(0, 255, 0));
        let area = coverage(&img);
        assert!((area - 800.0).abs() < 12.0, "{area}");

        let mut img = RgbaImage::new(64, 64);
        Painter::new(&mut img).fill_rounded_rect(8.0, 8.0, 40.0, 20.0, 6.0, rgb(0, 0, 255));
        let area = coverage(&img);
        let expected = 800.0 - (4.0 - std::f32::consts::PI) * 36.0;
        assert!((area - expected).abs() < 10.0, "{area} vs {expected}");
    }
}
