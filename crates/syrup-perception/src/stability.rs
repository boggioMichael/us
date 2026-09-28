//! What stays still while the game moves: the interface.
//!
//! For every pixel of the working image the map keeps how often it stayed
//! the same on frames where the rest of the screen changed, and how often it
//! sits on an edge. Pixels that are still *and* detailed are interface (a
//! plain sky that never changes is still, but it has no edges; a HUD has
//! borders, text and icons). Frames where nothing moved teach nothing about
//! stillness and are skipped.

use crate::pixels::WorkImage;

/// A pixel counts as changed when its luminance moved by more than this.
const CHANGED: i32 = 12;
/// A pixel counts as unchanged when its luminance moved by at most this.
const UNCHANGED: i32 = 6;
/// A frame "moved" when this fraction of its pixels changed (small, so that
/// games whose screen hardly moves, like card games, still teach the map).
const MOVING_FRAME: f32 = 0.005;

#[derive(Debug, Clone)]
pub struct StabilityMap {
    pub w: usize,
    pub h: usize,
    prev: Vec<u8>,
    still: Vec<f32>,
    detail: Vec<f32>,
    activity: Vec<f32>,
    observed: Vec<u16>,
    /// This frame's changed pixels (vs the previous analysed frame).
    changed: Vec<bool>,
    pub moving_frames: u32,
    pub frames: u32,
    /// Fraction of pixels that changed on the last update.
    pub last_change: f32,
}

impl StabilityMap {
    pub fn new() -> Self {
        StabilityMap {
            w: 0,
            h: 0,
            prev: Vec::new(),
            still: Vec::new(),
            detail: Vec::new(),
            activity: Vec::new(),
            observed: Vec::new(),
            changed: Vec::new(),
            moving_frames: 0,
            frames: 0,
            last_change: 0.0,
        }
    }

    fn reset(&mut self, w: usize, h: usize) {
        *self = StabilityMap::new();
        self.w = w;
        self.h = h;
        self.still = vec![0.5; w * h];
        self.detail = vec![0.0; w * h];
        self.activity = vec![0.0; w * h];
        self.observed = vec![0; w * h];
        self.changed = vec![false; w * h];
    }

    /// Learns from one more analysed frame; returns the fraction that changed.
    pub fn update(&mut self, work: &WorkImage) -> f32 {
        if work.w != self.w || work.h != self.h || self.prev.len() != work.luma.len() {
            self.reset(work.w, work.h);
            self.prev = work.luma.clone();
            self.update_detail(work);
            self.frames = 1;
            return 1.0;
        }
        self.frames += 1;
        let n = work.luma.len();
        let mut changed = 0usize;
        for i in 0..n {
            let d = (work.luma[i] as i32 - self.prev[i] as i32).abs();
            let c = d > CHANGED;
            self.changed[i] = c;
            changed += c as usize;
        }
        let fraction = changed as f32 / n.max(1) as f32;
        self.last_change = fraction;
        for i in 0..n {
            let a = if self.changed[i] { 1.0 } else { 0.0 };
            self.activity[i] += 0.2 * (a - self.activity[i]);
        }
        if fraction >= MOVING_FRAME {
            self.moving_frames += 1;
            for i in 0..n {
                let d = (work.luma[i] as i32 - self.prev[i] as i32).abs();
                let unchanged = if d <= UNCHANGED { 1.0 } else { 0.0 };
                let o = self.observed[i].saturating_add(1);
                self.observed[i] = o;
                // Quick to learn at first, then a steady moving average.
                let alpha = (1.0 / o as f32).max(0.08);
                self.still[i] += alpha * (unchanged - self.still[i]);
            }
        }
        self.update_detail(work);
        self.prev.copy_from_slice(&work.luma);
        fraction
    }

    fn update_detail(&mut self, work: &WorkImage) {
        let (w, h) = (work.w, work.h);
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let i = y * w + x;
                let gx = (work.luma[i + 1] as i32 - work.luma[i - 1] as i32).abs();
                let gy = (work.luma[i + w] as i32 - work.luma[i - w] as i32).abs();
                let edge = if gx.max(gy) > 20 { 1.0 } else { 0.0 };
                self.detail[i] += 0.25 * (edge - self.detail[i]);
            }
        }
    }

    /// Enough moving frames seen to tell interface from scene.
    pub fn is_ready(&self) -> bool {
        self.moving_frames >= 5
    }

    pub fn still(&self, x: usize, y: usize) -> f32 {
        self.still.get(y * self.w + x).copied().unwrap_or(0.0)
    }

    pub fn changed_now(&self, x: usize, y: usize) -> bool {
        self.changed.get(y * self.w + x).copied().unwrap_or(false)
    }

    /// Statistics over a rectangle of work pixels: (still fraction, detail
    /// fraction, fraction that changed recently).
    pub fn stats(&self, x: usize, y: usize, w: usize, h: usize) -> (f32, f32, f32) {
        let (mut s, mut d, mut a, mut n) = (0usize, 0usize, 0f32, 0usize);
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                let i = yy * self.w + xx;
                s += (self.still[i] > 0.85 && self.observed[i] >= 4) as usize;
                d += (self.detail[i] > 0.3) as usize;
                a += self.activity[i];
                n += 1;
            }
        }
        if n == 0 {
            return (0.0, 0.0, 0.0);
        }
        (s as f32 / n as f32, d as f32 / n as f32, a / n as f32)
    }

    /// Interface cells on a grid of `cell` x `cell` work pixels: still and detailed.
    pub fn interface_cells(&self, cell: usize) -> (usize, usize, Vec<bool>) {
        let gw = self.w.div_ceil(cell);
        let gh = self.h.div_ceil(cell);
        let mut out = vec![false; gw * gh];
        if !self.is_ready() {
            return (gw, gh, out);
        }
        for gy in 0..gh {
            for gx in 0..gw {
                let (s, d, _) = self.stats(gx * cell, gy * cell, cell, cell);
                out[gy * gw + gx] = s >= 0.85 && d >= 0.06;
            }
        }
        (gw, gh, out)
    }
}

impl Default for StabilityMap {
    fn default() -> Self {
        StabilityMap::new()
    }
}

/// Connected groups of `true` cells (8-connected), as (x, y, w, h) in cells,
/// after closing one-cell gaps.
pub fn cell_components(
    gw: usize,
    gh: usize,
    cells: &[bool],
    min_cells: usize,
) -> Vec<(usize, usize, usize, usize)> {
    // Close: dilate then erode, so a dotted border or a gap between letters joins up.
    let dilate = |src: &[bool]| {
        let mut out = vec![false; src.len()];
        for y in 0..gh {
            for x in 0..gw {
                if src[y * gw + x] {
                    for (dx, dy) in [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx >= 0 && ny >= 0 && (nx as usize) < gw && (ny as usize) < gh {
                            out[ny as usize * gw + nx as usize] = true;
                        }
                    }
                }
            }
        }
        out
    };
    let erode = |src: &[bool]| {
        let mut out = vec![false; src.len()];
        for y in 0..gh {
            for x in 0..gw {
                let mut all = src[y * gw + x];
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < gw && (ny as usize) < gh {
                        all &= src[ny as usize * gw + nx as usize];
                    }
                }
                out[y * gw + x] = all;
            }
        }
        out
    };
    let closed: Vec<bool> = erode(&dilate(cells))
        .iter()
        .zip(cells)
        .map(|(a, b)| *a || *b)
        .collect();
    let mut label = vec![0u32; closed.len()];
    let mut out = Vec::new();
    let mut next = 0u32;
    for start in 0..closed.len() {
        if !closed[start] || label[start] != 0 {
            continue;
        }
        next += 1;
        let mut stack = vec![start];
        label[start] = next;
        let (mut x0, mut y0, mut x1, mut y1, mut count) = (usize::MAX, usize::MAX, 0, 0, 0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % gw, i / gw);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            count += cells[i] as usize;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx as usize >= gw || ny as usize >= gh {
                        continue;
                    }
                    let j = ny as usize * gw + nx as usize;
                    if closed[j] && label[j] == 0 {
                        label[j] = next;
                        stack.push(j);
                    }
                }
            }
        }
        if count >= min_cells {
            out.push((x0, y0, x1 - x0 + 1, y1 - y0 + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> WorkImage {
        let luma: Vec<u8> = (0..w * h).map(|i| f(i % w, i / w)).collect();
        WorkImage {
            w,
            h,
            scale: 1.0,
            rgb: luma.iter().map(|l| [*l, *l, *l]).collect(),
            luma,
        }
    }

    #[test]
    fn a_still_detailed_corner_is_interface_and_the_moving_rest_is_not() {
        let mut map = StabilityMap::new();
        for t in 0..12usize {
            let img = work(80, 45, |x, y| {
                if x < 20 && y < 10 {
                    // A HUD: fixed stripes.
                    if (x / 2) % 2 == 0 { 230 } else { 20 }
                } else {
                    // The scene: stripes scrolling every frame.
                    if ((x + t * 3) / 3) % 2 == 0 { 200 } else { 40 }
                }
            });
            map.update(&img);
        }
        assert!(map.is_ready());
        let (gw, gh, cells) = map.interface_cells(4);
        let comps = cell_components(gw, gh, &cells, 2);
        assert_eq!(comps.len(), 1, "{comps:?}");
        let (x, y, w, h) = comps[0];
        assert_eq!((x, y), (0, 0));
        assert!((4..=6).contains(&w) && (2..=3).contains(&h), "{comps:?}");
    }
}
