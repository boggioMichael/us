//! Keeping track of interface regions from frame to frame.
//!
//! Candidates come from the stability map (still, detailed areas) and the
//! bar detector. The tracker matches them to the regions it knows, gives each
//! a stable id, smooths its rectangle, confirms it after a few sightings,
//! forgets it after a few misses, and for bars keeps the longest container
//! seen, so a bar's fill is measured against its full length.

use image::RgbaImage;
use syrup_core::observation::ObservedEvent;
use syrup_core::util::fnv64;
use syrup_core::{Confidence, NormRect, Rect, UiKind, UiRegion};

use crate::bars::BarCandidate;
use crate::pixels::{chroma, color_distance, dhash};

#[derive(Debug, Clone)]
pub enum CandidateKind {
    /// Still and detailed: interface of some kind.
    Stable { still: f32, detail: f32, activity: f32 },
    /// A bar, and how still its place has been.
    Bar { bar: BarCandidate, still: f32 },
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub rect: Rect,
    pub kind: CandidateKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarTrack {
    /// The edge the fill grows from (x of the left edge, or of the right edge when it drains right).
    pub anchor: i32,
    pub drains_right: bool,
    /// The longest the container has been seen.
    pub max_len: u32,
    pub fill: f32,
    pub color: [u8; 3],
    pub min_fill: f32,
    pub max_fill: f32,
    pub track_seen: bool,
    pub track_color: Option<[u8; 3]>,
}

#[derive(Debug, Clone)]
pub struct TrackedRegion {
    pub id: u32,
    pub rect: Rect,
    pub bar: Option<BarTrack>,
    pub first_ms: u64,
    pub last_ms: u64,
    pub seen: u32,
    pub missed: u32,
    pub confirmed: bool,
    pub still: f32,
    pub detail: f32,
    pub activity: f32,
    pub appearance: u64,
    /// A hash of the pixels now, to notice when the content changes.
    pub content: u64,
    pub content_changed_ms: u64,
    pub text_lines: usize,
    /// Came from the game's profile rather than from this session.
    pub seeded: bool,
    pub kind: UiKind,
}

impl TrackedRegion {
    pub fn is_bar(&self) -> bool {
        self.bar.is_some()
    }
}

pub struct RegionTracker {
    pub regions: Vec<TrackedRegion>,
    next_id: u32,
    frame: (u32, u32),
}

const CONFIRM_AFTER: u32 = 3;
const FORGET_AFTER: u32 = 8;

fn bar_appearance(b: &BarTrack, norm: &NormRect) -> u64 {
    let hue_bucket = (crate::pixels::hue(b.color[0], b.color[1], b.color[2]) / 30.0) as u32;
    let key = format!("bar:{hue_bucket}:{}:{}:{}", (norm.y * 20.0) as u32, (norm.x * 10.0) as u32, b.drains_right);
    fnv64(key.as_bytes())
}

impl RegionTracker {
    pub fn new() -> Self {
        RegionTracker { regions: Vec::new(), next_id: 1, frame: (0, 0) }
    }

    pub fn reset(&mut self) {
        self.regions.clear();
        self.frame = (0, 0);
    }

    /// Regions a profile already knows, to be looked for from the first frame.
    pub fn seed(&mut self, id: u32, rect: Rect, bar_color: Option<[u8; 3]>, now_ms: u64) {
        if self.regions.iter().any(|r| r.id == id) {
            return;
        }
        self.next_id = self.next_id.max(id + 1);
        let bar = bar_color.map(|color| BarTrack {
            anchor: rect.x,
            drains_right: false,
            max_len: rect.w,
            fill: 0.0,
            color,
            min_fill: 1.0,
            max_fill: 0.0,
            track_seen: false,
            track_color: None,
        });
        self.regions.push(TrackedRegion {
            id,
            rect,
            bar,
            first_ms: now_ms,
            last_ms: now_ms,
            seen: 0,
            missed: 0,
            confirmed: false,
            still: 0.0,
            detail: 0.0,
            activity: 0.0,
            appearance: 0,
            content: 0,
            content_changed_ms: now_ms,
            text_lines: 0,
            seeded: true,
            kind: UiKind::Unknown,
        });
    }

    fn fresh_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Matches this frame's candidates; returns the regions to report and what changed.
    pub fn update(&mut self, img: &RgbaImage, candidates: &[Candidate], now_ms: u64) -> Vec<ObservedEvent> {
        let size = img.dimensions();
        if size != self.frame {
            if self.frame != (0, 0) {
                // New size: the rectangles scale with it.
                let (sx, sy) = (size.0 as f32 / self.frame.0 as f32, size.1 as f32 / self.frame.1 as f32);
                for r in self.regions.iter_mut() {
                    r.rect = r.rect.scale(sx, sy);
                }
            }
            self.frame = size;
        }
        let mut matched = vec![false; self.regions.len()];
        let mut events = Vec::new();
        for c in candidates {
            match &c.kind {
                CandidateKind::Bar { bar: b, still } => {
                    let found = self.regions.iter().enumerate().position(|(i, r)| {
                        !matched[i]
                            && r.bar.as_ref().is_some_and(|t| {
                                let same_row = overlap_1d(r.rect.y, r.rect.h, b.container.y, b.container.h) >= 0.5;
                                let anchor = if b.drains_right { b.container.right() } else { b.container.x };
                                let t_anchor = if t.drains_right { r.rect.right() } else { r.rect.x };
                                same_row
                                    && t.drains_right == b.drains_right
                                    && (anchor - t_anchor).abs() <= 6.max(r.rect.h as i32)
                                    && color_distance(t.color, b.color) < 70.0
                            })
                    });
                    match found {
                        Some(i) => {
                            matched[i] = true;
                            let r = &mut self.regions[i];
                            update_bar(r, b);
                            r.still = *still;
                            r.seen += 1;
                            r.missed = 0;
                            r.last_ms = now_ms;
                        }
                        None => {
                            let id = self.fresh_id();
                            let mut r = new_region(id, b.container, now_ms);
                            r.bar = Some(BarTrack {
                                anchor: if b.drains_right { b.container.right() } else { b.container.x },
                                drains_right: b.drains_right,
                                max_len: b.container.w,
                                fill: b.fraction,
                                color: b.color,
                                min_fill: b.fraction,
                                max_fill: b.fraction,
                                track_seen: b.track_seen,
                                track_color: b.track_color,
                            });
                            r.still = *still;
                            self.regions.push(r);
                            matched.push(true);
                        }
                    }
                }
                CandidateKind::Stable { still, detail, activity } => {
                    let found = self
                        .regions
                        .iter()
                        .enumerate()
                        .filter(|(i, r)| !matched[*i] && !r.is_bar())
                        .map(|(i, r)| (i, match_score(&r.rect, &c.rect)))
                        .filter(|(_, s)| *s >= 0.3)
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i);
                    let i = match found {
                        Some(i) => {
                            let r = &mut self.regions[i];
                            r.rect = smooth(r.rect, c.rect, if r.seen < 3 { 0.6 } else { 0.25 });
                            i
                        }
                        None => {
                            let id = self.fresh_id();
                            self.regions.push(new_region(id, c.rect, now_ms));
                            matched.push(false);
                            self.regions.len() - 1
                        }
                    };
                    matched[i] = true;
                    let r = &mut self.regions[i];
                    r.seen += 1;
                    r.missed = 0;
                    r.last_ms = now_ms;
                    r.still = *still;
                    r.detail = *detail;
                    r.activity = *activity;
                }
            }
        }
        // Bars not found this frame: empty (the track is still there) or gone.
        for (i, r) in self.regions.iter_mut().enumerate() {
            if matched.get(i).copied().unwrap_or(false) {
                continue;
            }
            if let Some(bar) = r.bar.as_mut()
                && r.seen > 0
                && bar.track_color.is_some_and(|t| looks_empty(img, r.rect, bar.color, t))
            {
                bar.fill = 0.0;
                bar.min_fill = 0.0;
                r.seen += 1;
                r.missed = 0;
                r.last_ms = now_ms;
                continue;
            }
            r.missed += 1;
        }
        // Confirm, forget.
        for r in self.regions.iter_mut() {
            if !r.confirmed && r.seen >= CONFIRM_AFTER {
                r.confirmed = true;
                let norm = r.rect.to_norm(size.0, size.1);
                r.appearance = match &r.bar {
                    Some(b) => bar_appearance(b, &norm),
                    None => dhash(img, r.rect),
                };
                events.push(ObservedEvent::RegionAppeared { region: r.id });
            }
        }
        let before: Vec<(u32, bool)> = self.regions.iter().map(|r| (r.id, r.confirmed)).collect();
        self.regions.retain(|r| r.missed <= if r.seeded && r.seen == 0 { u32::MAX } else { FORGET_AFTER });
        for (id, confirmed) in before {
            if confirmed && !self.regions.iter().any(|r| r.id == id) {
                events.push(ObservedEvent::RegionDisappeared { region: id });
            }
        }
        // Content hashes, and kinds.
        for r in self.regions.iter_mut() {
            if r.missed == 0 {
                let h = dhash(img, r.rect);
                if r.content != 0 && h != r.content {
                    r.content_changed_ms = now_ms;
                }
                r.content = h;
            }
            r.kind = classify(r, size);
        }
        events
    }

    /// The confirmed regions, currently in view, as the observation reports them.
    pub fn visible(&self) -> Vec<UiRegion> {
        let (w, h) = self.frame;
        self.regions
            .iter()
            .filter(|r| r.confirmed && r.missed == 0)
            .map(|r| {
                let age = (r.seen as f32 / 10.0).min(1.0);
                let conf = match &r.bar {
                    Some(b) if b.track_seen || b.max_fill - b.min_fill > 0.05 => 0.55 + 0.4 * age,
                    Some(_) => 0.35 + 0.3 * age,
                    None => 0.3 + 0.5 * age * r.still.max(0.5),
                };
                UiRegion {
                    id: r.id,
                    rect: r.rect,
                    norm: r.rect.to_norm(w, h),
                    kind: r.kind.clone(),
                    stability: r.still,
                    confidence: Confidence::new(conf),
                    appearance: r.appearance,
                }
            })
            .collect()
    }

    pub fn get(&self, id: u32) -> Option<&TrackedRegion> {
        self.regions.iter().find(|r| r.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut TrackedRegion> {
        self.regions.iter_mut().find(|r| r.id == id)
    }
}

impl Default for RegionTracker {
    fn default() -> Self {
        RegionTracker::new()
    }
}

fn new_region(id: u32, rect: Rect, now_ms: u64) -> TrackedRegion {
    TrackedRegion {
        id,
        rect,
        bar: None,
        first_ms: now_ms,
        last_ms: now_ms,
        seen: 0,
        missed: 0,
        confirmed: false,
        still: 0.0,
        detail: 0.0,
        activity: 0.0,
        appearance: 0,
        content: 0,
        content_changed_ms: now_ms,
        text_lines: 0,
        seeded: false,
        kind: UiKind::Unknown,
    }
}

fn overlap_1d(a: i32, al: u32, b: i32, bl: u32) -> f32 {
    let lo = a.max(b);
    let hi = (a + al as i32).min(b + bl as i32);
    if hi <= lo {
        return 0.0;
    }
    (hi - lo) as f32 / al.min(bl).max(1) as f32
}

/// IoU, or how much of the smaller one the bigger contains (regions grow and shrink as the map learns).
fn match_score(a: &Rect, b: &Rect) -> f32 {
    let iou = a.iou(b);
    let inter = a.intersection(b).map(|r| r.area()).unwrap_or(0) as f32;
    let small = a.area().min(b.area()).max(1) as f32;
    iou.max(0.8 * inter / small)
}

fn smooth(old: Rect, new: Rect, a: f32) -> Rect {
    let l = |o: i32, n: i32| (o as f32 + (n - o) as f32 * a).round() as i32;
    let x0 = l(old.x, new.x);
    let y0 = l(old.y, new.y);
    let x1 = l(old.right(), new.right());
    let y1 = l(old.bottom(), new.bottom());
    Rect::from_corners(x0, y0, x1, y1)
}

fn update_bar(r: &mut TrackedRegion, b: &BarCandidate) {
    let Some(t) = r.bar.as_mut() else { return };
    t.color = [
        ((t.color[0] as u32 * 3 + b.color[0] as u32) / 4) as u8,
        ((t.color[1] as u32 * 3 + b.color[1] as u32) / 4) as u8,
        ((t.color[2] as u32 * 3 + b.color[2] as u32) / 4) as u8,
    ];
    t.track_seen |= b.track_seen;
    if b.track_color.is_some() {
        t.track_color = b.track_color;
    }
    let len = b.container.w.max(t.max_len);
    // The container is only trusted to grow when a track was seen or the fill itself is longer.
    if b.track_seen || b.fill.w > t.max_len {
        t.max_len = len;
    }
    t.fill = (b.fill.w as f32 / t.max_len.max(1) as f32).clamp(0.0, 1.0);
    t.min_fill = t.min_fill.min(t.fill);
    t.max_fill = t.max_fill.max(t.fill);
    let rect = if t.drains_right {
        Rect::new(t.anchor - t.max_len as i32, b.container.y, t.max_len, b.container.h)
    } else {
        Rect::new(t.anchor, b.container.y, t.max_len, b.container.h)
    };
    r.rect = smooth(r.rect, rect, 0.5);
    t.anchor = if t.drains_right { r.rect.right() } else { r.rect.x };
}

/// The bar's place shows no fill at all, just its track.
fn looks_empty(img: &RgbaImage, rect: Rect, fill: [u8; 3], track: [u8; 3]) -> bool {
    let Some(r) = rect.clip(img.width(), img.height()) else {
        return false;
    };
    if r.w < 4 || r.h < 2 {
        return false;
    }
    let (mut vivid, mut like_track, mut n) = (0, 0, 0);
    let y = r.y + r.h as i32 / 2;
    let mut x = r.x;
    while x < r.right() {
        let p = img.get_pixel(x as u32, y as u32).0;
        let c = [p[0], p[1], p[2]];
        vivid += (chroma(p[0], p[1], p[2]) >= 48 && color_distance(c, fill) < 70.0) as u32;
        like_track += (color_distance(c, track) < 40.0) as u32;
        n += 1;
        x += (r.w as i32 / 32).max(1);
    }
    vivid * 10 < n && like_track * 10 >= n * 7
}

/// What a region probably is, from its shape, place and behaviour.
fn classify(r: &TrackedRegion, (fw, fh): (u32, u32)) -> UiKind {
    if let Some(b) = &r.bar {
        return UiKind::Bar { fill: b.fill, color: b.color, vertical: false };
    }
    let (w, h) = (r.rect.w as f32, r.rect.h as f32);
    let (fw, fh) = (fw.max(1) as f32, fh.max(1) as f32);
    let aspect = w / h.max(1.0);
    let (cx, cy) = r.rect.center();
    let near_corner = (cx / fw < 0.3 || cx / fw > 0.7) && (cy / fh < 0.35 || cy / fh > 0.65);
    if (0.5..=2.6).contains(&aspect)
        && h >= fh * 0.08
        && h <= fh * 0.4
        && near_corner
        && r.activity > 0.004
        && r.activity < 0.2
        && r.detail > 0.1
    {
        return UiKind::Minimap;
    }
    if r.text_lines >= 2 {
        return UiKind::TextPanel;
    }
    if (0.6..=1.7).contains(&aspect) && w <= fw * 0.06 && h <= fh * 0.1 && w >= 8.0 {
        return UiKind::Icon;
    }
    if w * h >= fw * fh * 0.01 {
        return UiKind::Panel;
    }
    UiKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(x: i32, fill_w: u32, total: u32, track: bool) -> Candidate {
        let fill = Rect::new(x, 500, fill_w, 20);
        let container = if track { Rect::new(x, 500, total, 20) } else { fill };
        Candidate {
            rect: container,
            kind: CandidateKind::Bar {
                bar: BarCandidate {
                    fill,
                    container,
                    fraction: fill_w as f32 / container.w as f32,
                    color: [200, 40, 40],
                    hue: 0.0,
                    drains_right: false,
                    track_seen: track,
                    track_color: track.then_some([25, 22, 28]),
                },
                still: 1.0,
            },
        }
    }

    #[test]
    fn a_bar_keeps_its_id_and_its_full_length() {
        let img = RgbaImage::from_pixel(960, 540, image::Rgba([10, 10, 10, 255]));
        let mut t = RegionTracker::new();
        let mut events = Vec::new();
        events.extend(t.update(&img, &[bar(100, 200, 200, false)], 0));
        events.extend(t.update(&img, &[bar(100, 150, 200, true)], 100));
        events.extend(t.update(&img, &[bar(100, 200, 200, false)], 200));
        events.extend(t.update(&img, &[bar(100, 100, 200, true)], 300));
        let v = t.visible();
        assert_eq!(v.len(), 1);
        match v[0].kind {
            UiKind::Bar { fill, .. } => assert!((fill - 0.5).abs() < 0.02, "{fill}"),
            ref k => panic!("{k:?}"),
        }
        assert_eq!(events.iter().filter(|e| matches!(e, ObservedEvent::RegionAppeared { .. })).count(), 1);
        // Gone for a while: forgotten, with an event.
        let mut gone = Vec::new();
        for i in 0..10 {
            gone.extend(t.update(
                &RgbaImage::from_pixel(960, 540, image::Rgba([200, 200, 200, 255])),
                &[],
                400 + i * 100,
            ));
        }
        assert!(gone.iter().any(|e| matches!(e, ObservedEvent::RegionDisappeared { .. })));
        assert!(t.visible().is_empty());
    }
}
