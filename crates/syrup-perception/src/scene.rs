//! Which screen this is (gameplay, menu, loading, dialogue, cutscene,
//! defeat, victory), whole-frame measurements, and the scene signature.
//!
//! The classifier weighs universal cues: words on screen ("YOU DIED",
//! "LOADING", "NEW GAME", "VICTORY" and their relatives, bigger text counting
//! more), motion, interface present or not, letterbox bars, a panel over the
//! middle of the screen, colour draining away. A label changes only when the
//! new one is strong or has held for two analyses.

use syrup_core::observation::{FrameMetrics, ObservedEvent, SceneLabel, SceneSignature, TextItem};
use syrup_core::{Confidence, Rect, SceneKind};

use crate::pixels::{WorkImage, chroma};

pub const DEFEAT_WORDS: &[&str] = &[
    "you died",
    "you have died",
    "you are dead",
    "game over",
    "defeat",
    "defeated",
    "you lose",
    "you lost",
    "wasted",
    "mission failed",
    "died",
    "knocked out",
    "try again",
    "respawn",
    "revive",
];
pub const VICTORY_WORDS: &[&str] = &[
    "victory",
    "you win",
    "you won",
    "winner",
    "level complete",
    "stage clear",
    "stage cleared",
    "mission complete",
    "quest complete",
    "cleared",
    "congratulations",
    "well done",
];
pub const LOADING_WORDS: &[&str] = &["loading", "now loading", "please wait", "connecting", "saving"];
pub const MENU_WORDS: &[&str] = &[
    "new game",
    "continue",
    "load game",
    "options",
    "settings",
    "quit",
    "exit",
    "start game",
    "press start",
    "press any key",
    "play",
    "credits",
    "main menu",
    "single player",
    "multiplayer",
    "resume",
    "rules",
    "controls",
];
pub const DIALOGUE_WORDS: &[&str] =
    &["ok", "cancel", "yes", "no", "accept", "decline", "next", "close", "skip", "confirm"];

/// Lower-case words of `text`, with single spaces.
fn words(text: &str) -> String {
    syrup_core::util::normalize_words(text)
}

/// Whether the phrase occurs in `hay` as whole words.
pub fn has_phrase(hay: &str, phrase: &str) -> bool {
    let hay = format!(" {hay} ");
    hay.contains(&format!(" {phrase} "))
}

/// Phrases of `list` found in `text`.
pub fn phrases_in<'a>(text: &str, list: &[&'a str]) -> Vec<&'a str> {
    let w = words(text);
    list.iter().filter(|p| has_phrase(&w, p)).copied().collect()
}

/// Whole-frame measurements from the working image.
pub fn metrics(work: &WorkImage, change: f32) -> FrameMetrics {
    let n = work.rgb.len().max(1) as f32;
    let (mut b, mut s, mut r) = (0f32, 0f32, 0f32);
    for (c, l) in work.rgb.iter().zip(&work.luma) {
        b += *l as f32;
        s += chroma(c[0], c[1], c[2]) as f32;
        r += (c[0] as f32 - (c[1] as f32 + c[2] as f32) / 2.0).max(0.0);
    }
    let mut edges = 0usize;
    for y in 1..work.h.saturating_sub(1) {
        for x in 1..work.w.saturating_sub(1) {
            let i = y * work.w + x;
            let gx = (work.luma[i + 1] as i32 - work.luma[i - 1] as i32).abs();
            let gy = (work.luma[i + work.w] as i32 - work.luma[i - work.w] as i32).abs();
            edges += (gx.max(gy) > 20) as usize;
        }
    }
    FrameMetrics {
        change,
        motion: 0.0,
        brightness: b / n / 255.0,
        saturation: s / n / 255.0,
        red_tint: r / n / 255.0,
        detail: edges as f32 / n,
    }
}

/// Black bars above and below the picture (a cutscene's letterbox).
pub fn letterboxed(work: &WorkImage) -> bool {
    let row_dark = |y: usize| {
        let row = &work.luma[y * work.w..(y + 1) * work.w];
        row.iter().all(|l| *l < 16)
    };
    let top = (0..work.h).take_while(|y| row_dark(*y)).count();
    let bottom = (0..work.h).rev().take_while(|y| row_dark(*y)).count();
    let min = (work.h as f32 * 0.08) as usize;
    top >= min && bottom >= min && top + bottom < work.h * 2 / 3
}

/// What the scene looks like, without its interface: a difference hash and a colour histogram.
pub fn signature(work: &WorkImage, interface: &dyn Fn(usize, usize) -> bool) -> SceneSignature {
    let mut cells = [[(0f32, 0f32); 9]; 8];
    let mut hist = [0f32; 16];
    let mut total = 0f32;
    for y in 0..work.h {
        for x in 0..work.w {
            if interface(x, y) {
                continue;
            }
            let i = y * work.w + x;
            let (cx, cy) = ((x * 9 / work.w).min(8), (y * 8 / work.h).min(7));
            cells[cy][cx].0 += work.luma[i] as f32;
            cells[cy][cx].1 += 1.0;
            let c = work.rgb[i];
            let ch = chroma(c[0], c[1], c[2]) as f32 / 255.0;
            if ch > 0.15 {
                let hb = ((crate::pixels::hue(c[0], c[1], c[2]) / 30.0) as usize).min(11);
                hist[hb] += ch;
                total += ch;
            }
            let bb = (work.luma[i] as usize * 4 / 256).min(3);
            hist[12 + bb] += 0.5;
            total += 0.5;
        }
    }
    let mut hash = 0u64;
    for (cy, row) in cells.iter().enumerate() {
        for cx in 0..8 {
            let a = row[cx].0 / row[cx].1.max(1.0);
            let b = row[cx + 1].0 / row[cx + 1].1.max(1.0);
            if a < b {
                hash |= 1 << (cy * 8 + cx);
            }
        }
    }
    if total > 0.0 {
        for h in hist.iter_mut() {
            *h /= total;
        }
    }
    SceneSignature { hash, histogram: hist }
}

/// Everything the classifier weighs.
pub struct SceneFeatures<'a> {
    pub metrics: FrameMetrics,
    pub text: &'a [TextItem],
    pub frame: (u32, u32),
    pub interface_regions: usize,
    pub bars: usize,
    pub letterbox: bool,
    /// Panels over the middle of the screen with text in them.
    pub center_panels: Vec<Rect>,
    /// Extra words a plugin says mean a particular scene.
    pub extra: &'a [(SceneKind, String)],
    pub now_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SceneTracker {
    pub current: SceneLabel,
    candidate: Option<(SceneKind, u32, u64)>,
    prev: Option<FrameMetrics>,
    sat_avg: f32,
    red_avg: f32,
    drained: bool,
    analysed: u32,
}

impl SceneTracker {
    pub fn new() -> Self {
        SceneTracker::default()
    }

    /// The label for this frame, and the scene-level events.
    pub fn update(&mut self, f: &SceneFeatures) -> (SceneLabel, Vec<ObservedEvent>) {
        let mut events = Vec::new();
        let m = f.metrics;
        self.analysed += 1;
        // Flash, and the colour draining away or the screen going red (deaths
        // often look like that), measured against the usual look.
        if let Some(p) = self.prev
            && (m.brightness - p.brightness).abs() > 0.25
        {
            events.push(ObservedEvent::Flash { brightness_delta: m.brightness - p.brightness });
        }
        let warmed = self.analysed > 3;
        let drained = warmed && self.sat_avg > 0.1 && m.saturation < self.sat_avg * 0.45;
        let reddened = warmed && m.red_tint > self.red_avg + 0.08;
        // A hit flashes red while everything moves; a death screen goes red or
        // grey and holds still.
        let death_look = (drained || reddened) && m.change < 0.05;
        if death_look && !self.drained {
            events.push(ObservedEvent::ColorDrain { red: m.red_tint, saturation: m.saturation });
        }
        self.drained = death_look;
        // The usual look is learned from ordinary frames only (not from hits' red flashes).
        if !drained && !reddened {
            let a = if self.analysed <= 1 { 1.0 } else { 0.05 };
            self.sat_avg += a * (m.saturation - self.sat_avg);
            self.red_avg += a * (m.red_tint - self.red_avg);
        }
        self.prev = Some(m);

        let (kind, conf, reason) = classify(f, death_look);
        let label = if kind == self.current.kind {
            self.candidate = None;
            SceneLabel { kind, confidence: Confidence::new(conf.max(self.current.confidence.value() * 0.8)), reason }
        } else {
            let (held, since) = match self.candidate {
                Some((k, n, since)) if k == kind => (n + 1, since),
                _ => (1, f.now_ms),
            };
            self.candidate = Some((kind, held, since));
            // A strong label switches at once; a weak one must hold for two
            // analyses and over a second (a hit's red flash is not a death).
            let lasted = f.now_ms.saturating_sub(since) >= 1000;
            if conf >= 0.75
                || (held >= 2 && (lasted || conf >= 0.6 && kind != SceneKind::Defeat))
                || self.current.kind == SceneKind::Unknown
            {
                self.candidate = None;
                if self.current.kind != SceneKind::Unknown || kind != SceneKind::Unknown {
                    events.push(ObservedEvent::SceneChanged { from: self.current.kind, to: kind });
                }
                SceneLabel { kind, confidence: Confidence::new(conf), reason }
            } else {
                self.current.clone()
            }
        };
        self.current = label.clone();
        (label, events)
    }
}

fn classify(f: &SceneFeatures, drained: bool) -> (SceneKind, f32, String) {
    let m = f.metrics;
    let fh = f.frame.1.max(1) as f32;
    // Words, bigger text counting more.
    let mut all = String::new();
    let mut big = String::new();
    for t in f.text {
        all.push_str(&t.text);
        all.push('\n');
        if t.rect.h as f32 >= fh * 0.055 {
            big.push_str(&t.text);
            big.push('\n');
        }
    }
    let mut best: (SceneKind, f32, String) = (SceneKind::Unknown, 0.2, "nothing to go on yet".into());
    let mut consider = |k: SceneKind, c: f32, why: String| {
        if c > best.1 {
            best = (k, c, why);
        }
    };
    for (list, kind) in
        [(DEFEAT_WORDS, SceneKind::Defeat), (VICTORY_WORDS, SceneKind::Victory), (LOADING_WORDS, SceneKind::Loading)]
    {
        let big_hits = phrases_in(&big, list);
        let hits = phrases_in(&all, list);
        if let Some(p) = big_hits.first() {
            consider(kind, 0.9, format!("big \"{p}\" on screen"));
        } else if let Some(p) = hits.first() {
            let c = if kind == SceneKind::Loading { 0.6 } else { 0.55 };
            consider(kind, c + if f.center_panels.is_empty() { 0.0 } else { 0.15 }, format!("\"{p}\" on screen"));
        }
    }
    for (kind, word) in f.extra {
        if has_phrase(&words(&all), &words(word)) {
            consider(*kind, 0.8, format!("\"{word}\" (from the game's plugin)"));
        }
    }
    let menu = phrases_in(&all, MENU_WORDS);
    if menu.len() >= 2 {
        consider(SceneKind::Menu, 0.8, format!("menu words: {}", menu.join(", ")));
    } else if menu.len() == 1 && m.change < 0.02 && f.bars == 0 {
        consider(SceneKind::Menu, 0.5, format!("\"{}\" on a still screen", menu[0]));
    }
    if !f.center_panels.is_empty() {
        let buttons = phrases_in(&all, DIALOGUE_WORDS);
        let c = if buttons.is_empty() { 0.5 } else { 0.7 };
        consider(
            SceneKind::Dialogue,
            c,
            format!(
                "a panel with text over the middle{}",
                if buttons.is_empty() { String::new() } else { format!(" ({})", buttons.join(", ")) }
            ),
        );
    }
    if drained {
        consider(SceneKind::Defeat, 0.6, "the colour drained or the screen went red".into());
    }
    if m.brightness < 0.1 && m.detail < 0.03 && m.change < 0.02 {
        consider(SceneKind::Loading, 0.5, "dark, plain and still".into());
    }
    if f.letterbox && m.change > 0.01 {
        consider(SceneKind::Cutscene, 0.65, "letterbox bars and motion".into());
    }
    if f.interface_regions > 0 && m.change > 0.005 {
        let c = (0.6 + 0.05 * f.bars as f32).min(0.8);
        consider(SceneKind::Gameplay, c, format!("{} interface elements and motion", f.interface_regions));
    } else if m.change > 0.05 {
        consider(SceneKind::Gameplay, 0.45, "motion".into());
    }
    if m.change < 0.005 && f.text.len() >= 3 && f.bars == 0 && f.interface_regions == 0 {
        consider(SceneKind::Menu, 0.45, "a still screen of text".into());
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str, h: u32) -> TextItem {
        TextItem {
            text: t.into(),
            rect: Rect::new(100, 100, 300, h),
            region: None,
            confidence: Confidence::new(0.9),
            engine: "test".into(),
            fresh: true,
        }
    }

    fn features<'a>(text: &'a [TextItem], change: f32, regions: usize) -> SceneFeatures<'a> {
        SceneFeatures {
            metrics: FrameMetrics { change, brightness: 0.4, saturation: 0.3, ..Default::default() },
            text,
            frame: (960, 540),
            interface_regions: regions,
            bars: regions.min(1),
            letterbox: false,
            center_panels: Vec::new(),
            extra: &[],
            now_ms: 0,
        }
    }

    #[test]
    fn words_and_motion_decide_the_scene() {
        let mut t = SceneTracker::new();
        let died = [text("YOU DIED", 60)];
        let (l, _) = t.update(&features(&died, 0.1, 3));
        assert_eq!(l.kind, SceneKind::Defeat);
        let menu = [text("NEW GAME", 20), text("OPTIONS", 20), text("QUIT", 20)];
        let (l, ev) = t.update(&features(&menu, 0.0, 0));
        assert_eq!(l.kind, SceneKind::Menu);
        assert!(
            ev.iter()
                .any(|e| matches!(e, ObservedEvent::SceneChanged { from: SceneKind::Defeat, to: SceneKind::Menu }))
        );
        // Gameplay needs two analyses in a row to take over from a strong label.
        let (l, _) = t.update(&features(&[], 0.2, 4));
        assert_eq!(l.kind, SceneKind::Menu);
        let (l, _) = t.update(&features(&[], 0.2, 4));
        assert_eq!(l.kind, SceneKind::Gameplay);
    }

    #[test]
    fn phrases_match_whole_words_only() {
        assert!(has_phrase("you died again", "you died"));
        assert!(!has_phrase("studied", "died"));
        assert_eq!(phrases_in("NEW GAME\nOptions", MENU_WORDS), vec!["new game", "options"]);
    }
}
