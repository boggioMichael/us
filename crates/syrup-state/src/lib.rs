//! From observations to game state: what each element *is*, its value, and
//! what just changed.
//!
//! Every interface element and every number read on screen gets a track: its
//! history, the words next to it, its colour and place, and how it behaves
//! (a bar that falls during fights and is empty on the death screen; a
//! number that ticks down once a second; a counter that jumps when things
//! are defeated). Each track accumulates evidence for each concept, and the
//! engine assigns concepts with a confidence and the evidence behind them.
//! A player's correction, or an element learned in an earlier session,
//! outranks inference; a plugin can add or confirm concepts.

pub mod concepts;
pub mod values;

use std::collections::{BTreeMap, VecDeque};

use syrup_core::observation::{ObservedEvent, SceneSignature, TextItem};
use syrup_core::state::{Activity, ConceptUnit, Hypothesis};
use syrup_core::{
    ConceptValue, Confidence, GameState, NormRect, Observation, Rect, Reliability, SceneKind, Transition,
    TransitionKind, Trend, UiKind,
};

use crate::concepts::{OBJECTIVE_WORDS, color_priors, concept_for_label, spec};
use crate::values::{Reading, parse};

/// Something a profile or plugin already knows about an element.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementHint {
    pub norm: NormRect,
    /// `bar`, `text`, ...
    pub kind: String,
    pub concept: String,
    pub confidence: f32,
    /// The player said so: final.
    pub corrected: bool,
}

/// A concept newly believed with enough confidence to remember.
#[derive(Debug, Clone, PartialEq)]
pub struct ConceptLearned {
    pub concept: String,
    pub source: String,
    pub kind: String,
    pub norm: NormRect,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StateUpdate {
    pub transitions: Vec<Transition>,
    pub learned: Vec<ConceptLearned>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadingKind {
    Ratio,
    Percent,
    Time,
    Level,
    Pair,
    Count,
}

#[derive(Debug, Clone)]
struct Track {
    key: String,
    is_bar: bool,
    rect: Rect,
    norm: NormRect,
    hue: f32,
    chroma: u8,
    labels: BTreeMap<String, u32>,
    history: VecDeque<(u64, f64)>,
    /// For bars: the latest number read on or beside it (value, max, when).
    text: Option<(f64, Option<f64>, u64)>,
    raw_text: Option<String>,
    reading: Option<ReadingKind>,
    pair_other: Option<f64>,
    last_ms: u64,
    behavior: BTreeMap<&'static str, f32>,
    reasons: BTreeMap<&'static str, Vec<String>>,
    hint: Option<(String, f32, bool)>,
    step_times: VecDeque<(u64, f64)>,
    announced: Option<String>,
}

impl Track {
    fn new(key: String, is_bar: bool, rect: Rect, norm: NormRect) -> Self {
        Track {
            key,
            is_bar,
            rect,
            norm,
            hue: 0.0,
            chroma: 0,
            labels: BTreeMap::new(),
            history: VecDeque::new(),
            text: None,
            raw_text: None,
            reading: None,
            pair_other: None,
            last_ms: 0,
            behavior: BTreeMap::new(),
            reasons: BTreeMap::new(),
            hint: None,
            step_times: VecDeque::new(),
            announced: None,
        }
    }

    fn value(&self) -> Option<f64> {
        self.history.back().map(|(_, v)| *v)
    }

    fn add(&mut self, concept: &'static str, weight: f32, reason: &str) {
        let b = self.behavior.entry(concept).or_insert(0.0);
        *b = (*b + weight).clamp(-1.0, 1.6);
        let r = self.reasons.entry(concept).or_default();
        if !r.iter().any(|x| x == reason) {
            r.push(reason.to_string());
            if r.len() > 6 {
                r.remove(0);
            }
        }
    }

    fn push(&mut self, ts: u64, v: f64) {
        self.history.push_back((ts, v));
        while self.history.len() > 240 || self.history.front().is_some_and(|(t, _)| ts.saturating_sub(*t) > 120_000) {
            self.history.pop_front();
        }
        self.last_ms = ts;
    }

    /// The value `ms` ago (the latest sample at or before then).
    fn value_at(&self, ts: u64) -> Option<f64> {
        self.history.iter().rev().find(|(t, _)| *t <= ts).map(|(_, v)| *v)
    }

    fn trend(&self, now: u64) -> Trend {
        let (Some(v), Some(old)) = (self.value(), self.value_at(now.saturating_sub(2000))) else {
            return Trend::Unknown;
        };
        let scale = if self.is_bar { 1.0 } else { old.abs().max(1.0) };
        let d = (v - old) / scale;
        if d > 0.02 {
            Trend::Rising
        } else if d < -0.02 {
            Trend::Falling
        } else {
            Trend::Steady
        }
    }

    /// Evidence for each concept, all sources together.
    fn scores(&self, frame: (u32, u32)) -> Vec<(&'static str, f32, Vec<String>)> {
        let mut s: BTreeMap<&'static str, (f32, Vec<String>)> = BTreeMap::new();
        let mut add = |c: &'static str, w: f32, why: String| {
            let e = s.entry(c).or_insert((0.0, Vec::new()));
            e.0 += w;
            if w > 0.0 {
                e.1.push(why);
            }
        };
        for (label, n) in &self.labels {
            if let Some(c) = concept_for_label(label) {
                // Two letters ("HP", "EN") are also what OCR makes of noise: they
                // count fully once they have been read a few times.
                let reads = (*n as f32).min(5.0);
                let w = if label.chars().count() <= 2 { 0.5 + 0.15 * reads } else { 1.2 + 0.1 * reads.ln_1p() };
                add(c, w, format!("labelled \"{}\"", label.to_uppercase()));
            }
        }
        if self.is_bar {
            for (c, w) in color_priors(self.hue, self.chroma) {
                add(c, w, format!("{} bar", color_word(self.hue, self.chroma)));
            }
            let n = self.norm;
            if n.w > 0.7 && n.h < 0.03 && (n.y < 0.06 || n.y + n.h > 0.94) {
                add("experience", 0.45, "thin bar across the whole screen edge".into());
            }
            let (cx, cy) = n.center();
            if n.y < 0.15 && (0.3..0.7).contains(&cx) && n.w > 0.25 {
                add("boss_health", 0.4, "wide bar at the top centre".into());
            }
            // The player's own gauges sit along the edges; a bar in the middle of
            // the picture belongs to something in the world (an enemy, an effect).
            if (0.2..0.8).contains(&cx) && (0.18..0.78).contains(&cy) {
                for c in ["health", "mana", "stamina", "energy", "shield", "experience"] {
                    add(c, -0.5, "in the middle of the screen".into());
                }
            }
        }
        match self.reading {
            Some(ReadingKind::Time) => add("timer", 0.9, "reads as a time".into()),
            Some(ReadingKind::Level) => add("level", 1.4, "reads \"Lv\"".into()),
            Some(ReadingKind::Pair) => add("score", 0.6, "two numbers, like a score".into()),
            Some(ReadingKind::Percent) => add("experience", 0.15, "a percentage".into()),
            Some(ReadingKind::Ratio) if !self.is_bar => add("health", 0.1, "reads as current/maximum".into()),
            _ => {}
        }
        if let Some((c, conf, corrected)) = &self.hint
            && let Some(spec) = spec(c)
        {
            let w = if *corrected { 6.0 } else { 0.4 + 0.8 * conf };
            add(
                spec.name,
                w,
                if *corrected { "the player said so".into() } else { "learned in an earlier session".into() },
            );
        }
        for (c, w) in &self.behavior {
            let why = self.reasons.get(c).cloned().unwrap_or_default();
            let e = s.entry(c).or_insert((0.0, Vec::new()));
            e.0 += w;
            if *w > 0.0 {
                e.1.extend(why);
            }
        }
        let _ = frame;
        let fraction_only = |c: &str| matches!(spec(c).map(|s| s.unit), Some(ConceptUnit::Fraction));
        s.into_iter()
            .filter(|(c, _)| {
                if self.is_bar {
                    fraction_only(c) || *c == "timer"
                } else {
                    *c != "boss_health"
                        && (!fraction_only(c)
                            || self.reading == Some(ReadingKind::Ratio)
                            || self.reading == Some(ReadingKind::Percent))
                }
            })
            .map(|(c, (w, why))| (c, w, why))
            .collect()
    }
}

fn color_word(hue: f32, chroma: u8) -> &'static str {
    if chroma < 60 {
        return "grey";
    }
    match hue {
        h if !(20.0..340.0).contains(&h) => "red",
        h if h < 45.0 => "orange",
        h if h < 70.0 => "yellow",
        h if h < 160.0 => "green",
        h if h < 200.0 => "cyan",
        h if h < 250.0 => "blue",
        h if h < 300.0 => "purple",
        _ => "pink",
    }
}

fn hue_chroma(c: [u8; 3]) -> (f32, u8) {
    let (r, g, b) = (c[0] as f32, c[1] as f32, c[2] as f32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, d as u8)
}

/// Confidence from accumulated evidence.
fn confidence(score: f32) -> f32 {
    (1.0 - (-score.max(0.0)).exp()).clamp(0.0, 0.99)
}

pub struct StateEngine {
    tracks: BTreeMap<String, Track>,
    assigned: BTreeMap<String, String>,
    state: GameState,
    hints: Vec<ElementHint>,
    frame: (u32, u32),
    last_death_ms: Option<u64>,
    deaths: Vec<(u64, SceneSignature)>,
    places: Vec<(u64, SceneSignature)>,
    last_place_ms: u64,
    last_revisit_ms: u64,
    calm_since: Option<u64>,
    idle_flagged: bool,
    objective: Option<String>,
    level_up_ms: Option<u64>,
    /// Each concept's last value, whatever element it was read from (an
    /// element can be lost and found again as a new one between two reads).
    last_known: BTreeMap<String, ConceptValue>,
    /// Concepts already announced as learned, and where.
    announced: Vec<(String, NormRect)>,
    /// Minimum confidence for a concept to be reported.
    pub min_confidence: f32,
}

impl Default for StateEngine {
    fn default() -> Self {
        StateEngine::new()
    }
}

impl StateEngine {
    pub fn new() -> Self {
        StateEngine {
            tracks: BTreeMap::new(),
            assigned: BTreeMap::new(),
            state: GameState::default(),
            hints: Vec::new(),
            frame: (0, 0),
            last_death_ms: None,
            deaths: Vec::new(),
            places: Vec::new(),
            last_place_ms: 0,
            last_revisit_ms: 0,
            calm_since: None,
            idle_flagged: false,
            objective: None,
            level_up_ms: None,
            last_known: BTreeMap::new(),
            announced: Vec::new(),
            min_confidence: 0.35,
        }
    }

    /// What the profile or a plugin knows (replaces earlier hints).
    pub fn set_hints(&mut self, hints: Vec<ElementHint>) {
        self.hints = hints;
        for t in self.tracks.values_mut() {
            t.hint = None;
        }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// For plugins that add or confirm concepts after each update.
    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    /// The player says a concept is wrong: the element it was read from loses its claim to it.
    pub fn doubt(&mut self, concept: &str) {
        let Some(key) = self.assigned.get(concept).cloned() else { return };
        if let Some(t) = self.tracks.get_mut(&key)
            && let Some(spec) = spec(concept)
        {
            t.add(spec.name, -0.8, "the player said this was wrong");
            if t.hint.as_ref().is_some_and(|h| h.0 == concept && !h.2) {
                t.hint = None;
            }
        }
    }

    pub fn reset(&mut self) {
        let hints = std::mem::take(&mut self.hints);
        *self = StateEngine::new();
        self.hints = hints;
    }

    fn hint_for(&self, norm: &NormRect, is_bar: bool) -> Option<(String, f32, bool)> {
        self.hints
            .iter()
            .filter(|h| (h.kind == "bar") == is_bar && h.norm.iou(norm) > 0.4)
            .max_by(|a, b| a.corrected.cmp(&b.corrected).then(a.confidence.total_cmp(&b.confidence)))
            .map(|h| (h.concept.clone(), h.confidence, h.corrected))
    }

    fn transition(
        &mut self,
        out: &mut StateUpdate,
        ts: u64,
        kind: TransitionKind,
        subject: &str,
        from: Option<f64>,
        to: Option<f64>,
        detail: String,
        conf: f32,
    ) {
        let t = Transition {
            ts_ms: ts,
            kind,
            subject: subject.to_string(),
            from,
            to,
            detail,
            confidence: Confidence::new(conf),
        };
        self.state.recent.push(t.clone());
        out.transitions.push(t);
    }

    pub fn update(&mut self, obs: &Observation) -> StateUpdate {
        let mut out = StateUpdate::default();
        let now = obs.timestamp_ms;
        self.frame = obs.frame_size;
        let (fw, fh) = obs.frame_size;
        let prev_scene = self.state.scene;
        // Activity.
        let raw = (obs.metrics.motion * 4.0 + obs.metrics.change * 2.0).clamp(0.0, 1.0);
        let intensity = self.state.activity.intensity * 0.7 + raw * 0.3;
        if raw < 0.03 {
            self.calm_since.get_or_insert(now);
        } else {
            self.calm_since = None;
            self.idle_flagged = false;
        }
        self.state.activity =
            Activity { intensity, motion: obs.metrics.motion, idle_ms: self.calm_since.map(|t| now - t).unwrap_or(0) };
        self.state.timestamp_ms = now;
        self.state.scene = obs.scene.kind;
        let busy = intensity > 0.2;

        let level_up_text = obs.text.iter().any(|t| {
            let l = t.text.to_lowercase();
            l.contains("level up") || l.contains("leveled up") || l.contains("levelled up")
        });

        // Bars.
        let mut used_text = vec![false; obs.text.len()];
        for r in &obs.ui_regions {
            let UiKind::Bar { fill, color, .. } = r.kind else { continue };
            let key = format!("region:{}", r.id);
            let hint = self.hint_for(&r.norm, true);
            let t = self.tracks.entry(key.clone()).or_insert_with(|| Track::new(key.clone(), true, r.rect, r.norm));
            t.rect = r.rect;
            t.norm = r.norm;
            (t.hue, t.chroma) = hue_chroma(color);
            if hint.is_some() {
                t.hint = hint;
            }
            // Text on or beside the bar: its label and its number.
            for (i, item) in obs.text.iter().enumerate() {
                let inside = item.region == Some(r.id) || obs.labels_of(r.id).iter().any(|l| std::ptr::eq(*l, item));
                if !inside {
                    continue;
                }
                used_text[i] = true;
                for l in parse(&item.text) {
                    if let Some(lab) = &l.label {
                        *t.labels.entry(lab.clone()).or_insert(0) += 1;
                    }
                    if let Some(f) = l.reading.fraction() {
                        t.text = Some((l.reading.value(), l.reading.max(), now));
                        t.raw_text = Some(item.text.clone());
                        let _ = f;
                    }
                }
                for w in item.text.split(|c: char| !c.is_alphanumeric()) {
                    let w = w.to_lowercase();
                    if concept_for_label(&w).is_some() {
                        *t.labels.entry(w).or_insert(0) += 1;
                    }
                }
            }
            // The value: the text when it is fresh and agrees roughly, else the fill.
            let text_fraction = t.text.and_then(|(v, m, at)| {
                m.filter(|m| *m > 0.0 && now.saturating_sub(at) < 1500).map(|m| (v / m).clamp(0.0, 1.0))
            });
            let value = match text_fraction {
                Some(tf) if (tf - fill as f64).abs() < 0.2 => tf,
                _ => fill as f64,
            };
            let prev = t.value();
            t.push(now, value);
            if let Some(p) = prev {
                let d = value - p;
                if d <= -0.03 {
                    if busy {
                        t.add("health", 0.12, "falls while the action is busy");
                        t.add("shield", 0.04, "falls while the action is busy");
                    } else {
                        t.add("mana", 0.02, "falls when things are calm");
                        t.add("stamina", 0.02, "falls when things are calm");
                    }
                    t.step_times.push_back((now, d));
                    // Falling at a steady rate, sample after sample: a clock running down.
                    let recent: Vec<(u64, f64)> = t.step_times.iter().rev().take(6).copied().collect();
                    if recent.len() >= 5 {
                        let rates: Vec<f64> =
                            recent.windows(2).map(|w| w[0].1 / (w[0].0.saturating_sub(w[1].0).max(1) as f64)).collect();
                        let mean = rates.iter().sum::<f64>() / rates.len() as f64;
                        if mean < 0.0 && rates.iter().all(|r| (r - mean).abs() <= mean.abs() * 0.35) {
                            t.add("timer", 0.08, "drains at a steady rate");
                        }
                    }
                } else if (0.01..0.12).contains(&d) && !busy {
                    t.add("mana", 0.03, "refills slowly");
                    t.add("stamina", 0.03, "refills slowly");
                    t.add("experience", 0.02, "creeps up");
                } else if d >= 0.25 {
                    t.add("health", 0.04, "jumps back up (a potion, a respawn)");
                }
                // A timer bar drains in even steps and starts over full.
                if d > 0.5 {
                    let steps: Vec<f64> = t
                        .step_times
                        .iter()
                        .filter(|(ts, _)| now.saturating_sub(*ts) < 15_000)
                        .map(|(_, d)| *d)
                        .collect();
                    if steps.len() >= 4 {
                        let mut sorted = steps.clone();
                        sorted.sort_by(|a, b| a.total_cmp(b));
                        let median = sorted[sorted.len() / 2];
                        let even = steps.iter().filter(|s| (*s - median).abs() <= median.abs() * 0.6 + 0.02).count();
                        if even * 10 >= steps.len() * 7 {
                            t.add("timer", 0.35, "drains steadily and starts over full");
                            t.add("health", -0.2, "drains steadily and starts over full");
                        }
                    }
                    t.step_times.clear();
                }
                while t.step_times.len() > 30 {
                    t.step_times.pop_front();
                }
                // Experience resets when a level is gained.
                if (level_up_text || self.level_up_ms.is_some_and(|l| now.saturating_sub(l) < 3000))
                    && p > 0.6
                    && value < 0.35
                {
                    t.add("experience", 0.9, "went back to nearly empty at a level up");
                }
            }
        }

        // Numbers and words read elsewhere.
        let mut objective_text: Option<&TextItem> = None;
        for (i, item) in obs.text.iter().enumerate() {
            // Text that named a bar gave it its fraction; any other number in it still counts.
            let by_bar = used_text[i];
            let lower = item.text.to_lowercase();
            // Sentences and chat lines ("[All] Mika: ...") are not interface values.
            let readings = if is_speech(&item.text) { Vec::new() } else { parse(&item.text) };
            let tokens = item.text.split_whitespace().count();
            // "Defeat the Mossy King" is an objective; "DEFEAT" on its own is a
            // result screen, and chat about a quest is chat.
            let objective_like = OBJECTIVE_WORDS.iter().any(|w| crate::has_word(&lower, w));
            if objective_like && item.text.len() >= 6 && tokens >= 2 && !by_bar && !is_speech(&item.text) {
                objective_text = Some(item);
                continue;
            }
            for (n, l) in readings.iter().enumerate() {
                if by_bar && matches!(l.reading, Reading::Ratio { .. } | Reading::Percent(_)) {
                    continue;
                }
                // A plain number counts only in a short text ("GOLD 150"), not in a sentence.
                if matches!(l.reading, Reading::Count(_)) && tokens > 3 {
                    continue;
                }
                let label = l.label.clone().unwrap_or_default();
                // The same number keeps its key: by its label, else by where it is.
                let key = match (&l.label, item.region) {
                    (Some(lab), Some(r)) => format!("region:{r}/{lab}"),
                    (Some(lab), None) => format!("screen/{lab}"),
                    (None, Some(r)) => format!("region:{r}/#{n}"),
                    (None, None) => continue,
                };
                let norm = item.rect.to_norm(fw, fh);
                let hint = self.hint_for(&norm, false);
                // The middle of the screen is where the game happens: numbers there
                // (damage, prices, dialog text) are the world's, not the interface's,
                // unless Syrup already knows an element is there.
                let (cx, cy) = norm.center();
                let in_play = (0.25..0.75).contains(&cx) && (0.2..0.8).contains(&cy);
                if in_play && hint.is_none() && !(l.label.is_some() && item.region.is_some()) {
                    continue;
                }
                let t =
                    self.tracks.entry(key.clone()).or_insert_with(|| Track::new(key.clone(), false, item.rect, norm));
                t.rect = item.rect;
                t.norm = norm;
                if hint.is_some() {
                    t.hint = hint;
                }
                if !label.is_empty() {
                    *t.labels.entry(label.clone()).or_insert(0) += 1;
                }
                t.raw_text = Some(item.text.clone());
                t.reading = Some(match l.reading {
                    Reading::Ratio { .. } => ReadingKind::Ratio,
                    Reading::Percent(_) => ReadingKind::Percent,
                    Reading::Time(_) => ReadingKind::Time,
                    Reading::Level(_) => ReadingKind::Level,
                    Reading::Pair(_, b) => {
                        t.pair_other = Some(b);
                        ReadingKind::Pair
                    }
                    Reading::Count(_) => ReadingKind::Count,
                });
                let value = match l.reading {
                    Reading::Ratio { value, max } if max > 0.0 => {
                        t.text = Some((value, Some(max), now));
                        value
                    }
                    ref r => r.value(),
                };
                let prev = t.value();
                if prev == Some(value) {
                    t.push(now, value);
                    continue;
                }
                t.push(now, value);
                let Some(p) = prev else { continue };
                let d = value - p;
                let last_step = t.step_times.back().map(|(ts, _)| *ts);
                t.step_times.push_back((now, d));
                while t.step_times.len() > 12 {
                    t.step_times.pop_front();
                }
                if d.abs() == 1.0 {
                    // Once a second, steadily: a clock.
                    let regular = t.step_times.len() >= 3
                        && t.step_times.iter().rev().take(4).collect::<Vec<_>>().windows(2).all(|w| {
                            let gap = w[0].0.saturating_sub(w[1].0);
                            (700..=2600).contains(&gap) && w[0].1 == w[1].1
                        });
                    if regular {
                        t.add("timer", 0.3, "ticks by one about every second");
                    } else if d < 0.0 && busy {
                        t.add("ammo", 0.12, "goes down one at a time during action");
                    } else if d > 0.0 && last_step.is_none_or(|ls| now.saturating_sub(ls) > 15_000) {
                        t.add("level", 0.08, "goes up by one, rarely");
                        t.add("round", 0.08, "goes up by one, rarely");
                        t.add("kills", 0.05, "goes up by one, rarely");
                    }
                } else if d > 1.0 {
                    t.add("currency", 0.08, "jumps up");
                    t.add("score", 0.08, "jumps up");
                }
                if d > 0.0 && t.reading == Some(ReadingKind::Level) {
                    self.level_up_ms = Some(now);
                }
            }
        }

        // Deaths, wins, scenes.
        let scene_changed = obs.events.iter().find_map(|e| match e {
            ObservedEvent::SceneChanged { from, to } => Some((*from, *to)),
            _ => None,
        });
        if let Some((from, to)) = scene_changed {
            self.transition(
                &mut out,
                now,
                TransitionKind::SceneChanged,
                "scene",
                None,
                None,
                format!("{} → {}", from.word(), to.word()),
                obs.scene.confidence.value(),
            );
        }
        let entered_defeat = obs.scene.kind == SceneKind::Defeat && prev_scene != SceneKind::Defeat;
        if entered_defeat && self.last_death_ms.is_none_or(|t| now.saturating_sub(t) > 5000) {
            self.last_death_ms = Some(now);
            // The bar that just ran empty was health; one still full was not.
            for t in self.tracks.values_mut().filter(|t| t.is_bar) {
                let recent_min = t
                    .history
                    .iter()
                    .filter(|(ts, _)| now.saturating_sub(*ts) < 4000)
                    .map(|(_, v)| *v)
                    .fold(f64::MAX, f64::min);
                if recent_min <= 0.08 {
                    t.add("health", 0.9, "ran empty just before the death screen");
                } else if recent_min > 0.6 && now.saturating_sub(t.last_ms) < 4000 {
                    t.add("health", -0.25, "was still full at the death screen");
                }
            }
            let similar = self
                .deaths
                .iter()
                .filter(|(ts, sig)| now.saturating_sub(*ts) < 600_000 && sig.distance(&obs.signature) < 0.3)
                .count();
            self.deaths.push((now, obs.signature));
            let context =
                self.state.concepts.get("boss_health").map(|_| "during a boss fight".to_string()).unwrap_or_default();
            self.transition(
                &mut out,
                now,
                TransitionKind::PlayerDied,
                "player",
                None,
                None,
                context,
                obs.scene.confidence.value(),
            );
            if similar >= 2 {
                self.transition(
                    &mut out,
                    now,
                    TransitionKind::RepeatedFailure,
                    "player",
                    Some(similar as f64),
                    Some(similar as f64 + 1.0),
                    format!("death number {} around here", similar + 1),
                    0.7,
                );
            }
        }
        if obs.scene.kind == SceneKind::Victory && prev_scene != SceneKind::Victory {
            self.transition(
                &mut out,
                now,
                TransitionKind::Victory,
                "player",
                None,
                None,
                String::new(),
                obs.scene.confidence.value(),
            );
        }
        if level_up_text && self.level_up_ms.is_none_or(|t| now.saturating_sub(t) > 5000) {
            self.level_up_ms = Some(now);
            self.transition(
                &mut out,
                now,
                TransitionKind::LevelUp,
                "level",
                None,
                None,
                "\"level up\" on screen".into(),
                0.8,
            );
        }
        // Places: seen this before?
        if obs.scene.kind == SceneKind::Gameplay && now.saturating_sub(self.last_place_ms) >= 10_000 {
            self.last_place_ms = now;
            if now.saturating_sub(self.last_revisit_ms) > 60_000
                && self
                    .places
                    .iter()
                    .any(|(ts, sig)| now.saturating_sub(*ts) > 60_000 && sig.distance(&obs.signature) < 0.12)
            {
                self.last_revisit_ms = now;
                self.transition(&mut out, now, TransitionKind::AreaRevisited, "place", None, None, String::new(), 0.5);
            }
            self.places.push((now, obs.signature));
            if self.places.len() > 400 {
                self.places.remove(0);
            }
        }
        if obs.scene.kind == SceneKind::Gameplay && self.state.activity.idle_ms > 30_000 && !self.idle_flagged {
            self.idle_flagged = true;
            self.transition(
                &mut out,
                now,
                TransitionKind::Idle,
                "player",
                None,
                None,
                "nothing has moved for 30 seconds".into(),
                0.6,
            );
        }

        self.assign(now, &mut out);

        // The objective.
        if let Some(item) = objective_text {
            let words: String = item
                .text
                .chars()
                .filter(|c| c.is_alphabetic() || c.is_whitespace())
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let reading = parse(&item.text).into_iter().find_map(|l| match l.reading {
                Reading::Ratio { value, max } => Some((value, max)),
                _ => None,
            });
            let changed_words = self.objective.as_ref() != Some(&words);
            let old = self.state.concepts.get("objective").and_then(|c| c.value);
            if changed_words && self.objective.is_some() {
                self.transition(
                    &mut out,
                    now,
                    TransitionKind::ObjectiveChanged,
                    "objective",
                    None,
                    None,
                    item.text.clone(),
                    0.7,
                );
            } else if let (Some(o), Some((v, _))) = (old, reading)
                && v > o
            {
                self.transition(
                    &mut out,
                    now,
                    TransitionKind::CounterIncreased,
                    "objective",
                    Some(o),
                    Some(v),
                    item.text.clone(),
                    0.7,
                );
            }
            self.objective = Some(words);
            self.state.concepts.insert(
                "objective".into(),
                ConceptValue {
                    name: "objective".into(),
                    value: reading.map(|r| r.0),
                    max: reading.map(|r| r.1),
                    text: Some(item.text.clone()),
                    unit: ConceptUnit::Text,
                    source: item.region.map(|r| format!("region:{r}")).unwrap_or_else(|| "screen".into()),
                    confidence: Confidence::new(0.6),
                    reliability: Reliability::Heuristic,
                    trend: Trend::Unknown,
                    evidence: vec!["words like \"defeat\", \"collect\", \"quest\"".into()],
                    updated_ms: now,
                },
            );
        }

        // Keep the last minute of transitions.
        self.state.recent.retain(|t| now.saturating_sub(t.ts_ms) <= 60_000);
        out
    }

    fn assign(&mut self, now: u64, out: &mut StateUpdate) {
        let frame = self.frame;
        // Tracks not seen for a while stop counting.
        let live: Vec<&Track> = self.tracks.values().filter(|t| now.saturating_sub(t.last_ms) < 8000).collect();
        let mut options: Vec<(f32, String, &'static str, Vec<String>)> = Vec::new();
        for t in &live {
            for (c, score, why) in t.scores(frame) {
                let mut conf = confidence(score);
                // Keep what was assigned unless something clearly better comes.
                if self.assigned.get(c).is_some_and(|k| *k == t.key) {
                    conf += 0.1;
                }
                if conf >= self.min_confidence {
                    options.push((conf, t.key.clone(), c, why));
                }
            }
        }
        options.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut taken_tracks: Vec<String> = Vec::new();
        let mut assigned: BTreeMap<String, String> = BTreeMap::new();
        let mut concepts: BTreeMap<String, ConceptValue> = BTreeMap::new();
        for (conf, key, c, why) in options {
            if assigned.contains_key(c) || taken_tracks.contains(&key) {
                continue;
            }
            let t = &self.tracks[&key];
            taken_tracks.push(key.clone());
            assigned.insert(c.to_string(), key.clone());
            let spec = spec(c);
            let unit = spec.map(|s| s.unit).unwrap_or_default();
            let (value, max) = if t.is_bar {
                (t.value(), t.text.and_then(|(_, m, _)| m))
            } else {
                match (unit, t.text) {
                    (ConceptUnit::Fraction, Some((v, Some(m), _))) => (Some((v / m).clamp(0.0, 1.0)), Some(m)),
                    _ => (t.value(), None),
                }
            };
            let text_fresh = t.text.is_some_and(|(_, _, at)| now.saturating_sub(at) < 1500);
            let reliability = if t.is_bar && text_fresh {
                let tf = t.text.and_then(|(v, m, _)| m.map(|m| v / m.max(1e-9)));
                match (tf, t.value()) {
                    (Some(a), Some(b)) if (a - b).abs() < 0.08 => Reliability::Corroborated,
                    (Some(_), Some(_)) => Reliability::Heuristic,
                    _ => Reliability::Heuristic,
                }
            } else if now.saturating_sub(t.last_ms) > 1500 {
                Reliability::Predicted
            } else {
                Reliability::Heuristic
            };
            let mut evidence = why;
            evidence.dedup();
            let unit = if t.is_bar {
                ConceptUnit::Fraction
            } else if unit == ConceptUnit::Fraction {
                ConceptUnit::Count
            } else {
                unit
            };
            let value_cv = ConceptValue {
                name: c.to_string(),
                value,
                max,
                text: t.raw_text.clone(),
                unit: if t.is_bar || (t.text.is_some() && matches!(spec.map(|s| s.unit), Some(ConceptUnit::Fraction))) {
                    ConceptUnit::Fraction
                } else {
                    unit
                },
                source: key.clone(),
                confidence: Confidence::new(conf.min(0.99)),
                reliability,
                trend: t.trend(now),
                evidence,
                updated_ms: t.last_ms,
            };
            concepts.insert(c.to_string(), value_cv);
        }
        // Transitions on concepts.
        let old = std::mem::take(&mut self.state.concepts);
        for (name, cv) in &concepts {
            let resource = spec(name).is_some_and(|s| s.resource);
            // The same element's last value; or, when the element was found again
            // under a new id, the concept's last value if it is recent.
            let before = old
                .get(name)
                .filter(|o| o.source == cv.source)
                .or_else(|| self.last_known.get(name).filter(|o| now.saturating_sub(o.updated_ms) < 60_000));
            if let (Some(b), Some(now_f)) = (before.and_then(|b| b.fraction()), cv.fraction())
                && resource
                && cv.unit == ConceptUnit::Fraction
            {
                let conf = cv.confidence.value();
                if now_f < b - 0.1 {
                    self.transition(
                        out,
                        now,
                        TransitionKind::ResourceFell,
                        name,
                        Some(b),
                        Some(now_f),
                        String::new(),
                        conf,
                    );
                } else if now_f > b + 0.1 {
                    self.transition(
                        out,
                        now,
                        TransitionKind::ResourceRose,
                        name,
                        Some(b),
                        Some(now_f),
                        String::new(),
                        conf,
                    );
                }
                if now_f <= 0.02 && b > 0.02 {
                    self.transition(
                        out,
                        now,
                        TransitionKind::ResourceEmpty,
                        name,
                        Some(b),
                        Some(now_f),
                        String::new(),
                        conf,
                    );
                } else if now_f < 0.25 && b >= 0.25 {
                    self.transition(
                        out,
                        now,
                        TransitionKind::ResourceLow,
                        name,
                        Some(b),
                        Some(now_f),
                        String::new(),
                        conf,
                    );
                } else if now_f >= 0.5 && b < 0.25 {
                    self.transition(
                        out,
                        now,
                        TransitionKind::ResourceRecovered,
                        name,
                        Some(b),
                        Some(now_f),
                        String::new(),
                        conf,
                    );
                }
            } else if let (Some(b), Some(v)) = (before.and_then(|b| b.value), cv.value)
                && cv.unit != ConceptUnit::Fraction
                && (v - b).abs() >= 1.0
            {
                let kind = if v > b { TransitionKind::CounterIncreased } else { TransitionKind::CounterDecreased };
                let conf = cv.confidence.value();
                self.transition(out, now, kind, name, Some(b), Some(v), String::new(), conf);
                // A level goes up a little at a time; a big jump is a misread.
                if name == "level" && v > b && v - b <= 5.0 {
                    self.level_up_ms = Some(now);
                    self.transition(out, now, TransitionKind::LevelUp, "level", Some(b), Some(v), String::new(), conf);
                }
                if name == "round" && v > b {
                    self.transition(
                        out,
                        now,
                        TransitionKind::RoundStarted,
                        "round",
                        Some(b),
                        Some(v),
                        String::new(),
                        conf,
                    );
                }
            }
        }
        for (name, cv) in &concepts {
            if cv.value.is_some() || cv.text.is_some() {
                self.last_known.insert(name.clone(), cv.clone());
            }
        }
        // Newly learned concepts (once per concept and place: an element found
        // again, or read under a new id, is not news).
        for (name, cv) in &concepts {
            let t = self.tracks.get_mut(&cv.source).expect("assigned track exists");
            if cv.confidence.value() >= 0.55 && t.announced.as_deref() != Some(name) {
                t.announced = Some(name.clone());
                let norm = t.norm;
                if self.announced.iter().any(|(c, n)| c == name && n.iou(&norm) > 0.3) {
                    continue;
                }
                self.announced.push((name.clone(), norm));
                let t = self.tracks.get_mut(&cv.source).expect("assigned track exists");
                out.learned.push(ConceptLearned {
                    concept: name.clone(),
                    source: cv.source.clone(),
                    kind: if t.is_bar { "bar".into() } else { "text".into() },
                    norm: t.norm,
                    confidence: cv.confidence,
                    evidence: cv.evidence.clone(),
                });
            }
        }
        // Hypotheses: what is believed but not yet sure.
        self.state.hypotheses = concepts
            .values()
            .filter(|c| c.confidence.value() < 0.7)
            .map(|c| Hypothesis {
                statement: format!("{} is probably {}", c.source, c.name),
                confidence: c.confidence,
                evidence: c.evidence.clone(),
            })
            .collect();
        // The objective is kept by the text pass; everything else is replaced.
        let objective = old.get("objective").cloned();
        self.state.concepts = concepts;
        if let Some(o) = objective
            && now.saturating_sub(o.updated_ms) < 30_000
        {
            self.state.concepts.insert("objective".into(), o);
        }
        self.assigned = assigned;
    }
}

/// A line of chat or dialogue, or a sentence: not an interface value.
pub fn is_speech(text: &str) -> bool {
    static CHAT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let chat = CHAT.get_or_init(|| {
        regex::Regex::new(r"^\W*(\[[^\]]{1,14}\]\s*)?[\w .'-]{2,20}:\s+\S+\s+\S+").expect("valid pattern")
    });
    let words = text.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2).count();
    words > 5 || chat.is_match(text) || text.trim_start().starts_with('[')
}

/// Whether `word` appears in `text` as a word.
pub fn has_word(text: &str, word: &str) -> bool {
    let norm = syrup_core::util::normalize_words(text);
    format!(" {norm} ").contains(&format!(" {word} "))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod speech_tests {
    use super::is_speech;

    #[test]
    fn chat_is_not_a_hud_value() {
        assert!(is_speech("[All] Toph: selling mossy gems 5k ea"));
        assert!(is_speech("Mika: anyone for the boss?"));
        assert!(!is_speech("Lv. 12. syrupfan EXP 66.63%"));
        assert!(!is_speech("HP 1123/1480"));
        assert!(!is_speech("TIME 0:07"));
    }
}
