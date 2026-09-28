//! Rectangles in pixels and in fractions of the frame.
//!
//! Anything stored between sessions (a profile's UI elements, a HUD
//! signature) is stored as a [`NormRect`], so it survives a change of window
//! size or resolution; anything computed on one frame is a [`Rect`].

use serde::{Deserialize, Serialize};

/// An axis-aligned rectangle in pixels: top left corner, then size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }

    /// The rectangle spanning two corners, in either order.
    pub fn from_corners(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let (a, b) = (x0.min(x1), x0.max(x1));
        let (c, d) = (y0.min(y1), y0.max(y1));
        Rect::new(a, c, (b - a) as u32, (d - c) as u32)
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn center(&self) -> (f32, f32) {
        (
            self.x as f32 + self.w as f32 / 2.0,
            self.y as f32 + self.h as f32 / 2.0,
        )
    }

    /// Width over height (0 for an empty rectangle).
    pub fn aspect(&self) -> f32 {
        if self.h == 0 {
            0.0
        } else {
            self.w as f32 / self.h as f32
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    pub fn union(&self, other: &Rect) -> Rect {
        Rect::from_corners(
            self.x.min(other.x),
            self.y.min(other.y),
            self.right().max(other.right()),
            self.bottom().max(other.bottom()),
        )
    }

    /// Intersection over union, in [0, 1].
    pub fn iou(&self, other: &Rect) -> f32 {
        let inter = self.intersection(other).map_or(0, |r| r.area());
        let union = self.area() + other.area() - inter;
        if union == 0 {
            0.0
        } else {
            inter as f32 / union as f32
        }
    }

    /// Grown by `by` pixels on every side (shrunk when negative).
    pub fn inflate(&self, by: i32) -> Rect {
        let w = (self.w as i32 + 2 * by).max(0) as u32;
        let h = (self.h as i32 + 2 * by).max(0) as u32;
        Rect::new(self.x - by, self.y - by, w, h)
    }

    /// Clipped to a `width` x `height` frame; `None` if nothing is left.
    pub fn clip(&self, width: u32, height: u32) -> Option<Rect> {
        self.intersection(&Rect::new(0, 0, width, height))
    }

    /// Scaled by `sx`, `sy` (e.g. from a downscaled analysis frame back to full size).
    pub fn scale(&self, sx: f32, sy: f32) -> Rect {
        let x0 = (self.x as f32 * sx).floor() as i32;
        let y0 = (self.y as f32 * sy).floor() as i32;
        let x1 = (self.right() as f32 * sx).ceil() as i32;
        let y1 = (self.bottom() as f32 * sy).ceil() as i32;
        Rect::from_corners(x0, y0, x1, y1)
    }

    pub fn to_norm(&self, width: u32, height: u32) -> NormRect {
        let (fw, fh) = (width.max(1) as f32, height.max(1) as f32);
        NormRect {
            x: self.x as f32 / fw,
            y: self.y as f32 / fh,
            w: self.w as f32 / fw,
            h: self.h as f32 / fh,
        }
    }

    /// The gap between two rectangles along the axes (0 when they touch or overlap).
    pub fn gap(&self, other: &Rect) -> u32 {
        let dx = (other.x - self.right()).max(self.x - other.right()).max(0);
        let dy = (other.y - self.bottom())
            .max(self.y - other.bottom())
            .max(0);
        dx.max(dy) as u32
    }
}

/// A rectangle as fractions of the frame's width and height.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct NormRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl NormRect {
    pub fn to_rect(&self, width: u32, height: u32) -> Rect {
        let (fw, fh) = (width as f32, height as f32);
        Rect::from_corners(
            (self.x * fw).round() as i32,
            (self.y * fh).round() as i32,
            ((self.x + self.w) * fw).round() as i32,
            ((self.y + self.h) * fh).round() as i32,
        )
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Distance between centres, in frame fractions.
    pub fn center_distance(&self, other: &NormRect) -> f32 {
        let (a, b) = (self.center(), other.center());
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    pub fn iou(&self, other: &NormRect) -> f32 {
        // Compare on a fine fixed grid so the arithmetic matches `Rect::iou`.
        self.to_rect(10_000, 10_000)
            .iou(&other.to_rect(10_000, 10_000))
    }

    /// Which part of the screen it is in, for people: "top left", "bottom", "centre"...
    pub fn place(&self) -> &'static str {
        let (cx, cy) = self.center();
        let col = if cx < 0.34 {
            0
        } else if cx < 0.66 {
            1
        } else {
            2
        };
        let row = if cy < 0.34 {
            0
        } else if cy < 0.66 {
            1
        } else {
            2
        };
        [
            ["top left", "top", "top right"],
            ["left", "centre", "right"],
            ["bottom left", "bottom", "bottom right"],
        ][row][col]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_arithmetic() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersection(&b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!((a.iou(&b) - 25.0 / 175.0).abs() < 1e-6);
        assert_eq!(a.gap(&Rect::new(13, 0, 2, 2)), 3);
        assert_eq!(a.gap(&b), 0);
        assert_eq!(
            Rect::new(-5, -5, 10, 10).clip(8, 8),
            Some(Rect::new(0, 0, 5, 5))
        );
        assert_eq!(
            Rect::new(2, 3, 4, 5).scale(2.0, 2.0),
            Rect::new(4, 6, 8, 10)
        );
    }

    #[test]
    fn normalised_rects_survive_a_resize() {
        let r = Rect::new(128, 72, 256, 36);
        let n = r.to_norm(1280, 720);
        assert_eq!(n.to_rect(1280, 720), r);
        assert_eq!(n.to_rect(1920, 1080), Rect::new(192, 108, 384, 54));
        assert_eq!(
            Rect::new(0, 600, 100, 100).to_norm(1280, 720).place(),
            "bottom left"
        );
    }
}
