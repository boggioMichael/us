//! Universal perception: what is on the screen, with no game in mind.
//!
//! [`SceneAnalyzer::analyze`] turns a frame into an [`Observation`], the
//! universal intermediate representation:
//!
//! - **interface regions**, found because they stay still while the game
//!   moves ([`stability`]) and tracked with stable ids ([`regions`]);
//! - **bars** and how full they are ([`bars`]);
//! - **text**, read by the best OCR engine on the machine, scheduled so it
//!   stays cheap ([`text`]);
//! - **moving things** and which may be the player's character ([`motion`]);
//! - **the scene** (gameplay, menu, loading, dialogue, cutscene, defeat,
//!   victory), whole-frame measurements, a scene signature, and events
//!   ([`scene`]).
//!
//! Nothing here knows any game. What a region *means* (health, mana, a
//! score) is decided later, from evidence, by the game-state engine.

pub mod annotate;
pub mod bars;
pub mod motion;
pub mod pixels;
pub mod regions;
pub mod sampler;
pub mod scene;
pub mod stability;
pub mod text;

use std::sync::Arc;
use std::time::Instant;

use syrup_core::observation::{ElementRef, ObservedEvent, RelationKind, Relationship, SceneLabel, TextItem};
use syrup_core::{Frame, Observation, Rect, SceneKind, UiKind};

pub use annotate::{annotate, explain};
pub use sampler::{FrameSampler, SampleDecision, SamplerConfig};
pub use text::{OcrEngine, TextReaderConfig, best_engine, engine_named};

use crate::pixels::WorkImage;
use crate::regions::{Candidate, CandidateKind, RegionTracker};
use crate::scene::{SceneFeatures, SceneTracker};
use crate::stability::{StabilityMap, cell_components};
use crate::text::{RegionToRead, TextReader};

#[derive(Debug, Clone)]
pub struct PerceptionConfig {
    /// Width of the working copy used for stability, motion and signatures.
    pub work_width: usize,
    /// Stability cells, in working pixels.
    pub cell: usize,
    pub text: TextReaderConfig,
    /// Words a plugin says mean a particular scene.
    pub scene_words: Vec<(SceneKind, String)>,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        PerceptionConfig { work_width: 320, cell: 4, text: TextReaderConfig::default(), scene_words: Vec::new() }
    }
}

pub struct SceneAnalyzer {
    pub cfg: PerceptionConfig,
    stability: StabilityMap,
    pub regions: RegionTracker,
    text: TextReader,
    motion: motion::MotionTracker,
    scene: SceneTracker,
    last_region_text: std::collections::HashMap<u32, String>,
    /// Where bars were seen on the last few analyses, and their fill lengths.
    bar_history: std::collections::VecDeque<Vec<(Rect, u32)>>,
    analysed: u64,
}

impl SceneAnalyzer {
    pub fn new(cfg: PerceptionConfig, engine: Arc<dyn OcrEngine>) -> Self {
        let text = TextReader::new(engine, cfg.text.clone());
        SceneAnalyzer {
            cfg,
            stability: StabilityMap::new(),
            regions: RegionTracker::new(),
            text,
            motion: motion::MotionTracker::new(),
            scene: SceneTracker::new(),
            last_region_text: Default::default(),
            bar_history: Default::default(),
            analysed: 0,
        }
    }

    /// Forgets everything learned about the current screen (a different game, a new window).
    pub fn reset(&mut self) {
        self.stability = StabilityMap::new();
        self.regions.reset();
        self.text.reset();
        self.motion = motion::MotionTracker::new();
        self.scene = SceneTracker::new();
        self.last_region_text.clear();
        self.bar_history.clear();
        self.analysed = 0;
    }

    pub fn ocr_engine(&self) -> &'static str {
        self.text.engine_name()
    }

    pub fn ocr_reads(&self) -> u64 {
        self.text.reads
    }

    pub fn analysed(&self) -> u64 {
        self.analysed
    }

    pub fn analyze(&mut self, frame: &Frame) -> Observation {
        let started = Instant::now();
        let now = frame.timestamp_ms;
        let (fw, fh) = frame.size();
        let work = WorkImage::from_frame(&frame.image, self.cfg.work_width);
        let change = self.stability.update(&work);
        let mut metrics = scene::metrics(&work, change);
        self.analysed += 1;

        // Interface: still, detailed areas.
        let cell = self.cfg.cell.max(1);
        let (gw, gh, cells) = self.stability.interface_cells(cell);
        let mut candidates = Vec::new();
        for (cx, cy, cw, ch) in cell_components(gw, gh, &cells, 2) {
            let (x, y, w, h) =
                (cx * cell, cy * cell, (cw * cell).min(work.w - cx * cell), (ch * cell).min(work.h - cy * cell));
            let (still, detail, activity) = self.stability.stats(x, y, w, h);
            let rect = work.to_frame(x, y, w, h);
            // Whole-screen "components" are a still screen, not an element.
            if rect.area() as f32 > fw as f32 * fh as f32 * 0.6 {
                continue;
            }
            candidates.push(Candidate { rect, kind: CandidateKind::Stable { still, detail, activity } });
        }
        // Bars, kept when they are part of the interface.
        let ready = self.stability.is_ready();
        let mut seen_bars = Vec::new();
        for b in bars::find_bars(&frame.image) {
            let (x, y, w, h) = work.from_frame_rect(b.container);
            let (still, _, _) = self.stability.stats(x, y, w.max(1), h.max(1));
            let known =
                self.regions.regions.iter().any(|r| r.is_bar() && r.confirmed && r.rect.iou(&b.container) > 0.3);
            let big_enough = b.container.w as f32 >= fw as f32 * 0.04;
            // The same bar on earlier analyses: how often, and whether its fill changed.
            let past: Vec<u32> = self
                .bar_history
                .iter()
                .filter_map(|frame| frame.iter().find(|(c, _)| c.iou(&b.container) > 0.6).map(|(_, f)| *f))
                .collect();
            let varied = past.iter().any(|f| (*f as f32 - b.fill.w as f32).abs() > b.container.w as f32 * 0.02);
            seen_bars.push((b.container, b.fill.w));
            // Interface bars stay put while the scene moves; scenery that happens
            // to be a stripe (a ledge, a wall) does not. On a screen that never
            // moves, a bar is believed once it has stayed put and its fill changed.
            let interface = if ready {
                still >= 0.7 || (known && still >= 0.5)
            } else {
                known || (past.len() >= 2 && b.track_seen && varied)
            };
            if big_enough && interface {
                // A stable component that is just this bar is the bar.
                candidates
                    .retain(|c| !matches!(c.kind, CandidateKind::Stable { .. }) || c.rect.iou(&b.container) < 0.5);
                candidates.push(Candidate { rect: b.container, kind: CandidateKind::Bar { bar: b, still } });
            }
        }
        self.bar_history.push_back(seen_bars);
        while self.bar_history.len() > 6 {
            self.bar_history.pop_front();
        }
        let mut events = self.regions.update(&frame.image, &candidates, now);

        // Text.
        let visible_ids: Vec<u32> =
            self.regions.regions.iter().filter(|r| r.confirmed && r.missed == 0).map(|r| r.id).collect();
        let to_read: Vec<RegionToRead> = self
            .regions
            .regions
            .iter()
            .filter(|r| r.confirmed && r.missed == 0)
            .map(|r| RegionToRead { id: r.id, rect: r.rect, content: r.content, is_bar: r.is_bar() })
            .collect();
        let scene_cut = events.iter().any(|e| matches!(e, ObservedEvent::SceneChanged { .. })) || self.analysed <= 1;
        let big_change = change > 0.35;
        let mut text = self.text.update(frame, &to_read, scene_cut || big_change);
        let mut ui_regions = self.regions.visible();
        let mut relationships = Vec::new();
        assign_text(&mut text, &ui_regions, &mut relationships);
        // Text panels need their line counts.
        for id in &visible_ids {
            let lines = text.iter().filter(|t| t.region == Some(*id)).count();
            if let Some(r) = self.regions.get_mut(*id) {
                r.text_lines = lines;
            }
        }
        for r in ui_regions.iter_mut() {
            if let Some(t) = self.regions.get(r.id) {
                r.kind = t.kind.clone();
            }
        }
        // Text that changed, per region.
        for r in &ui_regions {
            let joined: Vec<&str> = text.iter().filter(|t| t.region == Some(r.id)).map(|t| t.text.as_str()).collect();
            if joined.is_empty() || !text.iter().any(|t| t.region == Some(r.id) && t.fresh) {
                continue;
            }
            let joined = joined.join(" | ");
            if self.last_region_text.get(&r.id) != Some(&joined) {
                if self.last_region_text.contains_key(&r.id) {
                    events.push(ObservedEvent::TextChanged { region: Some(r.id), text: joined.clone() });
                }
                self.last_region_text.insert(r.id, joined);
            }
        }

        // Motion, masked by the interface.
        let interface = |x: usize, y: usize| -> bool {
            let (cx, cy) = (x / cell, y / cell);
            if cx < gw && cy < gh && cells[cy * gw + cx] {
                return true;
            }
            let (fx, fy) = ((x as f32 * work.scale) as i32, (y as f32 * work.scale) as i32);
            ui_regions.iter().any(|r| r.rect.contains(fx, fy))
        };
        let (objects, characters, motion_fraction) = self.motion.update(&work, &interface);
        metrics.motion = motion_fraction;
        let signature = scene::signature(&work, &interface);

        // The scene.
        let center_panels = center_panels(&text, &ui_regions, (fw, fh));
        let bars = ui_regions.iter().filter(|r| matches!(r.kind, UiKind::Bar { .. })).count();
        let features = SceneFeatures {
            metrics,
            text: &text,
            frame: (fw, fh),
            interface_regions: ui_regions.len(),
            bars,
            letterbox: scene::letterboxed(&work),
            center_panels,
            extra: &self.cfg.scene_words,
            now_ms: now,
        };
        let (scene, scene_events) = self.scene.update(&features);
        events.extend(scene_events);

        let mut obs = Observation {
            frame_index: frame.index,
            timestamp_ms: now,
            frame_size: (fw, fh),
            scene,
            objects,
            text,
            ui_regions,
            characters,
            events,
            relationships,
            uncertainties: Vec::new(),
            metrics,
            signature,
            analysis_ms: 0.0,
        };
        if !self.text.available() {
            obs.uncertain("text", "no OCR engine on this computer (install Tesseract, or an OCR language pack on Windows): nothing on screen can be read");
        }
        if !ready {
            obs.uncertain(
                "interface",
                format!("still learning what stays put ({} of 5 moving frames)", self.stability.moving_frames.min(5)),
            );
        }
        obs.analysis_ms = started.elapsed().as_secs_f32() * 1000.0;
        obs
    }

    /// The scene label so far.
    pub fn scene(&self) -> &SceneLabel {
        &self.scene.current
    }
}

/// Puts each text item in the smallest region that holds it, and links
/// labels to the bars beside them.
fn assign_text(text: &mut [TextItem], regions: &[syrup_core::UiRegion], rel: &mut Vec<Relationship>) {
    for (i, t) in text.iter_mut().enumerate() {
        let area = t.rect.area().max(1) as f32;
        let holder = regions
            .iter()
            .filter(|r| r.rect.inflate(4).intersection(&t.rect).is_some_and(|x| x.area() as f32 >= area * 0.6))
            .min_by_key(|r| r.rect.area());
        if let Some(r) = holder {
            t.region = Some(r.id);
            rel.push(Relationship {
                from: ElementRef::Text(i),
                rel: RelationKind::Inside,
                to: ElementRef::Region(r.id),
            });
        }
        // A label beside (or just above) a bar names it.
        for r in regions.iter().filter(|r| matches!(r.kind, UiKind::Bar { .. })) {
            if r.rect.contains_rect(&t.rect) || Some(r.id) == t.region {
                continue;
            }
            let h = r.rect.h.max(8) as i32;
            let same_row = t.rect.y < r.rect.bottom() + h / 2 && t.rect.bottom() > r.rect.y - h / 2;
            let beside =
                same_row && ((r.rect.x - t.rect.right()).abs() <= h * 3 || (t.rect.x - r.rect.right()).abs() <= h * 3);
            let above = t.rect.bottom() <= r.rect.y + 2
                && r.rect.y - t.rect.bottom() <= h * 2
                && t.rect.right() > r.rect.x
                && t.rect.x < r.rect.right();
            if beside || above {
                rel.push(Relationship {
                    from: ElementRef::Text(i),
                    rel: RelationKind::Labels,
                    to: ElementRef::Region(r.id),
                });
            }
        }
    }
}

/// Panels over the middle of the screen with text in them: regions, or
/// stacks of text lines there.
fn center_panels(text: &[TextItem], regions: &[syrup_core::UiRegion], (fw, fh): (u32, u32)) -> Vec<Rect> {
    let (fw, fh) = (fw as f32, fh as f32);
    let central = |r: &Rect| {
        let (cx, cy) = r.center();
        cx > fw * 0.25 && cx < fw * 0.75 && cy > fh * 0.2 && cy < fh * 0.85
    };
    let mut out: Vec<Rect> = regions
        .iter()
        .filter(|r| matches!(r.kind, UiKind::Panel | UiKind::TextPanel | UiKind::Unknown))
        .filter(|r| {
            let a = r.rect.area() as f32 / (fw * fh);
            (0.03..0.6).contains(&a) && central(&r.rect) && text.iter().any(|t| t.region == Some(r.id))
        })
        .map(|r| r.rect)
        .collect();
    // Two or more lines stacked in the middle, not in any region.
    let mids: Vec<&TextItem> = text.iter().filter(|t| t.region.is_none() && central(&t.rect)).collect();
    if mids.len() >= 2 {
        let union = mids.iter().skip(1).fold(mids[0].rect, |a, t| a.union(&t.rect));
        if union.h as f32 <= fh * 0.5 && union.w as f32 <= fw * 0.7 {
            out.push(union);
        }
    }
    out
}

#[cfg(test)]
mod tests;
