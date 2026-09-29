//! Bars: health, mana, experience, timers, a boss's life. Found by what
//! they look like, never by where a particular game puts them.
//!
//! A bar's filled part is a long, thin, solid run of one vivid colour (a
//! little text printed over it is bridged). Its empty part is the track
//! beside it: darker or greyer, plain, ending at a border. The fill fraction
//! is the filled length over filled + empty. The tracker later keeps the
//! longest container seen, so a full bar with no visible track still
//! measures right once it has been seen less than full.

use image::RgbaImage;
use syrup_core::Rect;

use crate::pixels::{chroma, color_distance, hue, hue_distance, luma};

#[derive(Debug, Clone, PartialEq)]
pub struct BarCandidate {
    /// The filled part, in frame pixels.
    pub fill: Rect,
    /// Filled part and track together.
    pub container: Rect,
    /// Filled length over container length.
    pub fraction: f32,
    pub color: [u8; 3],
    pub hue: f32,
    /// The empty part is on the left (the bar drains towards the right).
    pub drains_right: bool,
    /// A track (empty part) was actually seen.
    pub track_seen: bool,
    /// The track's colour, when seen.
    pub track_color: Option<[u8; 3]>,
}

#[derive(Clone, Copy)]
struct Run {
    x0: usize,
    x1: usize,
    hue: f32,
}

struct Group {
    x0: usize,
    x1: usize,
    y0: usize,
    last_y: usize,
    rows: usize,
    covered: usize,
    hue_sum: f32,
    hue_n: f32,
    color_sum: [u64; 3],
    color_n: u64,
}

/// Vivid enough to be a bar's fill.
#[inline]
fn vivid(p: &[u8]) -> bool {
    chroma(p[0], p[1], p[2]) >= 70 && p[0].max(p[1]).max(p[2]) >= 90
}

/// Every bar-like shape in the frame.
pub fn find_bars(img: &RgbaImage) -> Vec<BarCandidate> {
    let (fw, fh) = (img.width() as usize, img.height() as usize);
    if fw < 32 || fh < 16 {
        return Vec::new();
    }
    // Work at most ~960 px wide: bars are big enough.
    let k = fw.div_ceil(960).max(1);
    let (w, h) = (fw / k, fh / k);
    let raw = img.as_raw();
    let px = |x: usize, y: usize| {
        let i = ((y * k) * fw + x * k) * 4;
        &raw[i..i + 3]
    };
    let min_len = (w / 80).max(8);
    let max_gap = 3usize.max(w / 240);
    let mut open: Vec<Group> = Vec::new();
    let mut done: Vec<Group> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    for y in 0..h {
        runs.clear();
        let mut x = 0;
        while x < w {
            let p = px(x, y);
            if !vivid(p) {
                x += 1;
                continue;
            }
            let (start, mut end, mut gap) = (x, x, 0usize);
            let mut hsum = hue(p[0], p[1], p[2]);
            let mut n = 1.0f32;
            let mut xx = x + 1;
            while xx < w {
                let q = px(xx, y);
                if vivid(q) && hue_distance(hue(q[0], q[1], q[2]), hsum / n) <= 20.0 {
                    end = xx;
                    gap = 0;
                    hsum += hue(q[0], q[1], q[2]);
                    n += 1.0;
                } else {
                    gap += 1;
                    if gap > max_gap {
                        break;
                    }
                }
                xx += 1;
            }
            if end + 1 - start >= min_len {
                runs.push(Run { x0: start, x1: end, hue: hsum / n });
            }
            x = end + 1;
        }
        // Runs join the group above them that they overlap (text printed over
        // a bar splits its rows into pieces; the pieces still join).
        let mut joined = vec![false; open.len()];
        let mut fresh: Vec<Group> = Vec::new();
        for r in &runs {
            let len = r.x1 + 1 - r.x0;
            let target = open.iter().position(|g| {
                let ov = (r.x1.min(g.x1) as i64 - r.x0.max(g.x0) as i64 + 1).max(0) as usize;
                y <= g.last_y + 2 && ov * 2 >= len && hue_distance(r.hue, g.hue_sum / g.hue_n) <= 20.0
            });
            let mid = px((r.x0 + r.x1) / 2, y);
            match target {
                Some(i) => {
                    let g = &mut open[i];
                    if g.last_y != y {
                        g.rows += 1;
                        g.last_y = y;
                    }
                    joined[i] = true;
                    g.x0 = g.x0.min(r.x0);
                    g.x1 = g.x1.max(r.x1);
                    g.covered += len;
                    g.hue_sum += r.hue;
                    g.hue_n += 1.0;
                    for c in 0..3 {
                        g.color_sum[c] += mid[c] as u64;
                    }
                    g.color_n += 1;
                }
                None => fresh.push(Group {
                    x0: r.x0,
                    x1: r.x1,
                    y0: y,
                    last_y: y,
                    rows: 1,
                    covered: len,
                    hue_sum: r.hue,
                    hue_n: 1.0,
                    color_sum: [mid[0] as u64, mid[1] as u64, mid[2] as u64],
                    color_n: 1,
                }),
            }
        }
        let mut i = 0;
        while i < open.len() {
            if y > open[i].last_y + 2 {
                done.push(open.swap_remove(i));
            } else {
                i += 1;
            }
        }
        open.extend(fresh);
    }
    done.append(&mut open);
    let max_h = (h / 15).max(3);
    let mut out = Vec::new();
    for g in done {
        let gh = g.last_y + 1 - g.y0;
        let gw = g.x1 + 1 - g.x0;
        if gh < 2
            || gh > max_h
            || gw * 2 < gh * 3
            || (g.rows as f32) < gh as f32 * 0.75
            || (g.covered as f32) < (gw * gh) as f32 * 0.6
        {
            continue;
        }
        let fill = Rect::new((g.x0 * k) as i32, (g.y0 * k) as i32, (gw * k) as u32, (gh * k) as u32);
        let color = [
            (g.color_sum[0] / g.color_n) as u8,
            (g.color_sum[1] / g.color_n) as u8,
            (g.color_sum[2] / g.color_n) as u8,
        ];
        if fill.h < 3 || !bounded(img, fill, color) || !solid(img, fill, g.hue_sum / g.hue_n) {
            continue;
        }
        let bar = measure_track(img, fill, color, g.hue_sum / g.hue_n);
        // Long and thin, counting the empty part.
        if bar.container.w < bar.container.h * 4 {
            continue;
        }
        out.push(bar);
    }
    // A bar inside another (a highlight stripe on a fill) is the same bar.
    out.sort_by_key(|b| std::cmp::Reverse(b.container.area()));
    let mut kept: Vec<BarCandidate> = Vec::new();
    for b in out {
        if !kept.iter().any(|k| {
            k.container.intersection(&b.container).is_some_and(|i| i.area() as f32 >= b.container.area() as f32 * 0.5)
        }) {
            kept.push(b);
        }
    }
    kept
}

/// A fill is solid: nearly all of it is the fill's colour (text drawn in a
/// vivid colour is not: its strokes leave gaps).
fn solid(img: &RgbaImage, fill: Rect, fill_hue: f32) -> bool {
    let (mut good, mut n) = (0u32, 0u32);
    let sx = (fill.w as i32 / 60).max(1);
    let sy = (fill.h as i32 / 8).max(1);
    let mut y = fill.y;
    while y < fill.bottom() {
        let mut x = fill.x;
        while x < fill.right() {
            let p = img.get_pixel(x as u32, y as u32).0;
            good += (vivid(&p[..3]) && hue_distance(hue(p[0], p[1], p[2]), fill_hue) <= 24.0) as u32;
            n += 1;
            x += sx;
        }
        y += sy;
    }
    good as f32 >= n as f32 * 0.7
}

/// A fill is a band: the rows just above and just below it are mostly
/// something else (a frame, a track, the interface around it), not more of
/// the same colour, as they would be for sky or a wall.
fn bounded(img: &RgbaImage, fill: Rect, color: [u8; 3]) -> bool {
    let (fw, fh) = (img.width() as i32, img.height() as i32);
    let same_fraction = |y: i32| {
        if y < 0 || y >= fh {
            return 0.0;
        }
        let step = (fill.w as i32 / 40).max(1);
        let (mut same, mut n) = (0, 0);
        let mut x = fill.x;
        while x < fill.right().min(fw) {
            let p = img.get_pixel(x as u32, y as u32).0;
            same += (color_distance([p[0], p[1], p[2]], color) < 45.0) as u32;
            n += 1;
            x += step;
        }
        same as f32 / n.max(1) as f32
    };
    let above = same_fraction(fill.y - 2).min(same_fraction(fill.y - 3));
    let below = same_fraction(fill.bottom() + 1).min(same_fraction(fill.bottom() + 2));
    above < 0.35 && below < 0.35
}

/// Median colour of one column of the bar, over its middle rows (text
/// printed over a bar is thin, so the median is what is behind it).
fn column(img: &RgbaImage, x: i32, y0: i32, y1: i32) -> [u8; 3] {
    let mut ch: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for y in y0..y1 {
        let p = img.get_pixel(x as u32, y as u32).0;
        for c in 0..3 {
            ch[c].push(p[c]);
        }
    }
    let mut out = [0u8; 3];
    for c in 0..3 {
        if ch[c].is_empty() {
            return out;
        }
        let mid = ch[c].len() / 2;
        out[c] = *ch[c].select_nth_unstable(mid).1;
    }
    out
}

/// The commonest colour of `n` columns starting at `x` going `dir` (text
/// printed on the track is the minority).
fn typical(img: &RgbaImage, x: i32, dir: i32, n: i32, y0: i32, y1: i32) -> Option<[u8; 3]> {
    let fw = img.width() as i32;
    let cols: Vec<[u8; 3]> =
        (0..n).map(|i| x + i * dir).filter(|x| *x >= 0 && *x < fw).map(|x| column(img, x, y0, y1)).collect();
    if cols.is_empty() {
        return None;
    }
    let key = |c: &[u8; 3]| (c[0] / 32, c[1] / 32, c[2] / 32);
    let mut counts: std::collections::HashMap<(u8, u8, u8), (u32, [u32; 3])> = std::collections::HashMap::new();
    for c in &cols {
        let e = counts.entry(key(c)).or_insert((0, [0; 3]));
        e.0 += 1;
        for i in 0..3 {
            e.1[i] += c[i] as u32;
        }
    }
    let (_, (n, sum)) = counts.into_iter().max_by_key(|(k, v)| (v.0, std::cmp::Reverse(*k)))?;
    Some([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8])
}

/// How far the track runs from the fill's end, in one direction.
fn track_length(img: &RgbaImage, fill: Rect, color: [u8; 3], dir: i32) -> (i32, [u8; 3]) {
    let fw = img.width() as i32;
    let inset = (fill.h as i32 / 4).max(0);
    let (y0, y1) = (fill.y + inset, fill.bottom() - inset.max(if fill.h > 2 { 1 } else { 0 }));
    if y1 <= y0 {
        return (0, [0; 3]);
    }
    let fill_l = luma(color[0], color[1], color[2]) as i32;
    let mut x = if dir > 0 { fill.right() } else { fill.x - 1 };
    // Skip a pixel or two of antialiasing at the fill's end.
    let mut skipped = 0;
    while skipped < 2 && x >= 0 && x < fw && color_distance(column(img, x, y0, y1), color) < 60.0 {
        x += dir;
        skipped += 1;
    }
    if x < 0 || x >= fw {
        return (0, [0; 3]);
    }
    // A full bar ends at its frame: a column of the border's colour (the border
    // above the fill's end), all the way down, going on above and below.
    {
        let fh = img.height() as i32;
        let pixel = |x: i32, y: i32| {
            let p = img.get_pixel(x as u32, y.clamp(0, fh - 1) as u32).0;
            [p[0], p[1], p[2]]
        };
        let end = if dir > 0 { fill.right() - 1 } else { fill.x };
        let c = column(img, x, y0, y1);
        // (A frame is thin: something else follows within a few columns; a
        // track of the same dark colour goes on.)
        let thin = (1..=4).any(|k| {
            let xx = x + k * dir;
            xx >= 0 && xx < fw && color_distance(column(img, xx, y0, y1), c) > 30.0
        });
        if thin
            && color_distance(c, color) > 60.0
            && color_distance(c, pixel(end, fill.y - 1)) < 30.0
            && color_distance(c, pixel(x, fill.y - 1)) < 30.0
            && color_distance(c, pixel(x, fill.bottom())) < 30.0
            && (fill.y..fill.bottom()).all(|y| color_distance(pixel(x, y), c) < 30.0)
        {
            return (0, [0; 3]);
        }
    }
    let Some(track) = typical(img, x, dir, 16, y0, y1) else {
        return (0, [0; 3]);
    };
    let track_l = luma(track[0], track[1], track[2]) as i32;
    // A track is darker than the fill, or greyer, and not the fill's colour.
    if !(track_l <= fill_l + 10 || chroma(track[0], track[1], track[2]) + 30 < chroma(color[0], color[1], color[2]))
        || color_distance(track, color) < 45.0
    {
        return (0, [0; 3]);
    }
    // A strongly coloured "track" is scenery next to the bar (lava beside a
    // full health bar), unless it is the fill's own colour, dimmed.
    let same_family = hue_distance(hue(track[0], track[1], track[2]), hue(color[0], color[1], color[2])) <= 12.0;
    if chroma(track[0], track[1], track[2]) >= 70 && !(same_family && track_l * 10 <= fill_l * 7) {
        return (0, [0; 3]);
    }
    // A track is a band like the fill: what is just above and below it is something else.
    let fh = img.height() as i32;
    let differs = |x: i32, y: i32| {
        y < 0 || y >= fh || {
            let p = img.get_pixel(x as u32, y as u32).0;
            color_distance([p[0], p[1], p[2]], track) > 30.0
        }
    };
    let banded = |x: i32| (1..=4).any(|d| differs(x, fill.y - d)) && (0..4).any(|d| differs(x, fill.bottom() + d));
    let max_len = (fill.w as i32 * 40).min(fw);
    let (mut len, mut odd, mut texty, mut steps) = (0, 0, 0, 0);
    let pixel = |x: i32, y: i32| {
        let p = img.get_pixel(x as u32, y.clamp(0, fh - 1) as u32).0;
        [p[0], p[1], p[2]]
    };
    // The border running along the top of the bar: above the fill's end to
    // start with, then above wherever the track was last seen.
    let end = if dir > 0 { fill.right() - 1 } else { fill.x };
    let mut border: Option<[u8; 3]> = Some(pixel(end, fill.y - 1));
    // Whether the column before was track: a track goes on through stretches
    // where the interface around it is as dark as it is (rightwards only).
    let mut on_track = false;
    while x >= 0 && x < fw && steps < max_len {
        let c = column(img, x, y0, y1);
        let track_colored = color_distance(c, track) <= 40.0;
        let is_banded = track_colored && banded(x);
        if is_banded || (track_colored && on_track && dir > 0) {
            len = steps + 1;
            odd = 0;
            texty = 0;
            on_track = true;
            if is_banded {
                border = Some(pixel(x, fill.y - 1));
            }
        } else {
            on_track = false;
            // The bar's frame: the colour of the border along the top of the bar,
            // going on above and below it (the border turning the corner). The
            // track ends there; text printed on a track is not that colour.
            // (One colour all the way down: a column of text strokes over the
            // track is light and dark in turn.)
            if !track_colored
                && color_distance(c, color) > 60.0
                && border.is_some_and(|b| color_distance(c, b) < 30.0)
                && color_distance(c, pixel(x, fill.y - 1)) < 30.0
                && color_distance(c, pixel(x, fill.bottom())) < 30.0
                && (fill.y..fill.bottom()).all(|y| color_distance(pixel(x, y), c) < 30.0)
            {
                break;
            }
            odd += 1;
            // Light, grey columns are text printed on the track ("22,496,313 (26.9%)");
            // track-coloured ones whose band cannot be seen are where the interface
            // around the bar happens to be as dark as the track. Runs of either,
            // as long as the fill or the track so far, are bridged.
            let text_like = luma(c[0], c[1], c[2]) as i32 > track_l + 50 && chroma(c[0], c[1], c[2]) < 70;
            // (Only rightwards, the way nearly every bar drains: leftwards the dark
            // interface beside a bar, and its light icons, would pass for more track.)
            texty += (text_like || track_colored) as i32;
            let allowed = if dir > 0 && texty * 2 >= odd {
                (fill.h as i32 / 2).max(4).max(len.min(400))
            } else {
                (fill.h as i32 / 2).max(4)
            };
            // A few odd columns are bridged; more end it.
            if odd > allowed {
                break;
            }
        }
        x += dir;
        steps += 1;
    }
    (len, track)
}

fn measure_track(img: &RgbaImage, fill: Rect, color: [u8; 3], hue: f32) -> BarCandidate {
    let (right, rc) = track_length(img, fill, color, 1);
    let (mut left, lc) = track_length(img, fill, color, -1);
    // Bars fill from the left almost always: a short "track" on the left only
    // is the gap before a full bar, not the empty part of one draining left.
    if right == 0 && (left as f32) < fill.w as f32 * 0.25 {
        left = 0;
    }
    let (container, drains_right, track, tc) = if right >= left {
        (Rect::new(fill.x, fill.y, fill.w + right as u32, fill.h), false, right, rc)
    } else {
        (Rect::new(fill.x - left, fill.y, fill.w + left as u32, fill.h), true, left, lc)
    };
    let track_seen = track >= 3;
    let container = if track_seen { container } else { fill };
    BarCandidate {
        fill,
        container,
        fraction: fill.w as f32 / container.w.max(1) as f32,
        color,
        hue,
        drains_right,
        track_seen,
        track_color: track_seen.then_some(tc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrup_paint::{Painter, rgb};

    fn scene_with_bar(fill: f32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(640, 360, image::Rgba([90, 110, 90, 255]));
        let mut p = Painter::new(&mut img);
        // A framed bar: border, dark track, red fill.
        p.fill_rect(100, 300, 204, 20, rgb(70, 70, 80));
        p.fill_rect(102, 302, 200, 16, rgb(25, 22, 28));
        p.fill_rect(102, 302, (200.0 * fill) as i32, 16, rgb(210, 40, 40));
        // Text printed over it.
        p.text(150.0, 302.0, "87/100", syrup_paint::FontStyle::bold(13.0), rgb(255, 255, 255));
        // Noise: a small red square is not a bar.
        p.fill_rect(400, 100, 20, 20, rgb(220, 30, 30));
        img
    }

    #[test]
    fn a_framed_bar_is_found_with_its_fill() {
        for fill in [0.3f32, 0.65, 0.9] {
            let bars = find_bars(&scene_with_bar(fill));
            assert_eq!(bars.len(), 1, "fill {fill}: {bars:?}");
            let b = &bars[0];
            assert!((b.fraction - fill).abs() < 0.04, "fill {fill}: measured {}", b.fraction);
            assert!(b.track_seen);
            assert!((b.container.x - 102).abs() <= 2 && (b.container.w as i32 - 200).abs() <= 4, "{:?}", b.container);
            assert!(b.color[0] > 150 && b.color[1] < 90);
        }
    }

    #[test]
    #[ignore]
    fn debug_bar() {
        let img = image::open(std::env::var("BAR_IMAGE").unwrap()).unwrap().to_rgba8();
        for b in find_bars(&img) {
            eprintln!("{b:?}");
        }
    }

    #[test]
    fn a_full_bar_has_no_track_yet() {
        let bars = find_bars(&scene_with_bar(1.0));
        assert_eq!(bars.len(), 1);
        assert!(!bars[0].track_seen || bars[0].fraction > 0.97);
    }
}
