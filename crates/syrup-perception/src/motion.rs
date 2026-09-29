//! Things that move: blobs of change, tracked across frames, and which one
//! might be the player's character.
//!
//! When the camera pans (side-scrollers follow the player; 3D games turn),
//! everything changes at once. The dominant shift between the two frames is
//! estimated first, from row and column profiles, and the difference is
//! taken against the shifted previous frame, so what is left moves on its
//! own. Changes on the interface are ignored.

use syrup::tracking::ObjectTracker;
use syrup_core::observation::{CharacterHypothesis, CharacterRole, ObservedObject};
use syrup_core::{Confidence, Rect};

use crate::pixels::WorkImage;

pub struct MotionTracker {
    prev: Vec<u8>,
    size: (usize, usize),
    tracker: ObjectTracker,
    /// The camera's last estimated shift, in working pixels.
    pub camera: (i32, i32),
}

/// Mean absolute difference of two profiles when `b` is shifted by `d`.
fn profile_error(a: &[f32], b: &[f32], d: i32) -> f32 {
    let n = a.len() as i32;
    let (mut s, mut c) = (0f32, 0f32);
    for i in 0..n {
        let j = i - d;
        if j >= 0 && j < n {
            s += (a[i as usize] - b[j as usize]).abs();
            c += 1.0;
        }
    }
    if c < n as f32 * 0.5 { f32::MAX } else { s / c }
}

fn best_shift(cur: &[f32], prev: &[f32], range: i32) -> i32 {
    let base = profile_error(cur, prev, 0);
    let (mut best, mut err) = (0, base);
    for d in -range..=range {
        let e = profile_error(cur, prev, d);
        if e < err {
            best = d;
            err = e;
        }
    }
    // Only a clearly better fit counts as the camera moving.
    if err < base * 0.8 { best } else { 0 }
}

impl MotionTracker {
    pub fn new() -> Self {
        MotionTracker { prev: Vec::new(), size: (0, 0), tracker: ObjectTracker::new(10.0, 3), camera: (0, 0) }
    }

    /// Moving things in this frame (frame coordinates), ignoring changes on the
    /// interface; the player-character candidates; the fraction of the frame
    /// covered by motion.
    pub fn update(
        &mut self,
        work: &WorkImage,
        interface: &dyn Fn(usize, usize) -> bool,
    ) -> (Vec<ObservedObject>, Vec<CharacterHypothesis>, f32) {
        let (w, h) = (work.w, work.h);
        if self.size != (w, h) || self.prev.len() != work.luma.len() {
            self.size = (w, h);
            self.prev = work.luma.clone();
            self.tracker = ObjectTracker::new(10.0, 3);
            return (Vec::new(), Vec::new(), 0.0);
        }
        let mask: Vec<bool> = (0..w * h).map(|i| interface(i % w, i / w)).collect();
        // Row and column profiles of the scene (interface left out).
        let profiles = |l: &[u8]| {
            let (mut cols, mut cn) = (vec![0f32; w], vec![0f32; w]);
            let (mut rows, mut rn) = (vec![0f32; h], vec![0f32; h]);
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    if !mask[i] {
                        cols[x] += l[i] as f32;
                        cn[x] += 1.0;
                        rows[y] += l[i] as f32;
                        rn[y] += 1.0;
                    }
                }
            }
            for (v, n) in cols.iter_mut().zip(&cn) {
                *v /= n.max(1.0);
            }
            for (v, n) in rows.iter_mut().zip(&rn) {
                *v /= n.max(1.0);
            }
            (cols, rows)
        };
        let (cc, cr) = profiles(&work.luma);
        let (pc, pr) = profiles(&self.prev);
        let dx = best_shift(&cc, &pc, (w as i32 / 8).max(4));
        let dy = best_shift(&cr, &pr, (h as i32 / 10).max(3));
        self.camera = (dx, dy);
        // What changed beyond the camera's move.
        const CELL: usize = 2;
        let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
        let mut cells = vec![0u8; gw * gh];
        let mut moved = 0usize;
        for y in 0..h {
            let py = y as i32 - dy;
            if py < 0 || py >= h as i32 {
                continue;
            }
            for x in 0..w {
                let px = x as i32 - dx;
                if px < 0 || px >= w as i32 || mask[y * w + x] {
                    continue;
                }
                let d = (work.luma[y * w + x] as i32 - self.prev[py as usize * w + px as usize] as i32).abs();
                if d > 30 {
                    cells[(y / CELL) * gw + x / CELL] += 1;
                    moved += 1;
                }
            }
        }
        self.prev.copy_from_slice(&work.luma);
        let on: Vec<bool> = cells.iter().map(|c| *c >= 2).collect();
        let comps = crate::stability::cell_components(gw, gh, &on, 2);
        let frame_cells = (gw * gh) as f32;
        let boxes: Vec<(usize, usize, usize, usize)> =
            comps.into_iter().filter(|(_, _, cw, ch)| ((cw * ch) as f32) < frame_cells * 0.25).collect();
        let detections: Vec<(f32, f32, f32, f32)> = boxes
            .iter()
            .map(|(x, y, bw, bh)| {
                (
                    ((x * CELL) as f32 + (bw * CELL) as f32 / 2.0),
                    ((y * CELL) as f32 + (bh * CELL) as f32 / 2.0),
                    (bw * CELL) as f32,
                    (bh * CELL) as f32,
                )
            })
            .collect();
        let tracks = self.tracker.update(&detections).to_vec();
        let mut objects = Vec::new();
        for t in tracks.iter() {
            let (bw, bh) = (t.width.max(1.0), t.height.max(1.0));
            let (x0, y0) = ((t.position.x - bw / 2.0).max(0.0), (t.position.y - bh / 2.0).max(0.0));
            let rect = Rect::new(
                (x0 * work.scale) as i32,
                (y0 * work.scale) as i32,
                (bw * work.scale) as u32,
                (bh * work.scale) as u32,
            );
            let (cx, cy) = ((t.position.x as usize).min(w - 1), (t.position.y as usize).min(h - 1));
            let c = work.rgb[cy * w + cx];
            let conf = (0.3 + 0.05 * t.age_frames.min(10) as f32) * if t.is_predicted() { 0.6 } else { 1.0 };
            objects.push(ObservedObject {
                id: t.id,
                rect,
                velocity: ((t.velocity.x + dx as f32) * work.scale, (t.velocity.y + dy as f32) * work.scale),
                age_frames: t.age_frames,
                predicted: t.is_predicted(),
                confidence: Confidence::new(conf),
                color: c,
            });
        }
        let characters = characters(&objects, w as f32 * work.scale, h as f32 * work.scale);
        (objects, characters, moved as f32 / (w * h).max(1) as f32)
    }
}

impl Default for MotionTracker {
    fn default() -> Self {
        MotionTracker::new()
    }
}

/// The long-lived moving thing near the middle is probably the player's own
/// character (the camera follows it); the others are something else.
fn characters(objects: &[ObservedObject], fw: f32, fh: f32) -> Vec<CharacterHypothesis> {
    let score = |o: &ObservedObject| {
        let (cx, cy) = o.rect.center();
        let central = 1.0 - ((cx / fw - 0.5).abs() * 2.0).min(1.0);
        let vertical = 1.0 - ((cy / fh - 0.55).abs() * 2.0).min(1.0);
        let size = o.rect.h as f32 / fh;
        let sized = if (0.04..0.35).contains(&size) { 1.0 } else { 0.4 };
        central * (0.5 + 0.5 * vertical) * sized * (o.age_frames.min(20) as f32 / 20.0)
    };
    let best =
        objects.iter().filter(|o| o.age_frames >= 4 && !o.predicted).max_by(|a, b| score(a).total_cmp(&score(b)));
    let mut out = Vec::new();
    if let Some(b) = best
        && score(b) > 0.25
    {
        out.push(CharacterHypothesis {
            object: b.id,
            role: CharacterRole::PlayerCandidate,
            confidence: Confidence::new(0.2 + 0.4 * score(b)),
            reason: "stays near the middle, where the camera follows".into(),
        });
    }
    for o in objects.iter().filter(|o| o.age_frames >= 3 && Some(o.id) != best.map(|b| b.id)) {
        out.push(CharacterHypothesis {
            object: o.id,
            role: CharacterRole::Other,
            confidence: Confidence::new(0.2),
            reason: "moves on its own".into(),
        });
    }
    out
}
