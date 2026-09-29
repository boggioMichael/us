//! Learning a game's profile while watching it.
//!
//! Each analysed frame teaches where the interface is (known UI elements,
//! the HUD signature used to recognise the game next time), which small
//! pictures recur (visual vocabulary), which words the game uses
//! (terminology), and each concept the state engine becomes sure of is
//! written onto its element. A player's correction is stored and outranks
//! everything. A plugin can seed the same fields.

use syrup_core::observation::ObservedEvent;
use syrup_core::profile::{Correction, KnownUiElement, ProfileItem, VisualWord};
use syrup_core::util::{hamming, now_iso};
use syrup_core::{Confidence, GameProfile, HudMark, NormRect, Observation, UiKind};
use syrup_state::{ConceptLearned, ElementHint};

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "you", "your", "are", "with", "this", "that", "from", "have", "not", "but", "was", "all",
    "can", "will", "has", "its", "our", "out", "one", "get", "got", "any", "how", "who", "what", "when", "where",
    "into", "more", "now", "new", "off", "see", "use", "yes", "lol",
];

fn kind_word(kind: &UiKind) -> &'static str {
    match kind {
        UiKind::Bar { .. } => "bar",
        UiKind::Minimap => "minimap",
        UiKind::TextPanel => "text_panel",
        UiKind::Icon => "icon",
        UiKind::Panel => "panel",
        UiKind::Unknown => "element",
    }
}

fn same_class(a: &str, b: &str) -> bool {
    (a == "bar") == (b == "bar")
}

#[derive(Debug, Default)]
pub struct ProfileLearner {
    last_ms: Option<u64>,
    /// Changes worth announcing since the last call to `take_changes`.
    changes: Vec<String>,
}

impl ProfileLearner {
    pub fn new() -> Self {
        ProfileLearner::default()
    }

    /// What changed in the profile since the last call (for `ProfileUpdated` events).
    pub fn take_changes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.changes)
    }

    /// Learns from one analysed frame.
    pub fn observe(&mut self, p: &mut GameProfile, obs: &Observation) {
        let now = obs.timestamp_ms;
        let dt = self.last_ms.map(|t| now.saturating_sub(t).min(2000)).unwrap_or(0);
        self.last_ms = Some(now);
        p.stats.observed_ms += dt;
        p.stats.analysed_frames += 1;
        let iso = now_iso();
        if p.stats.first_seen.is_none() {
            p.stats.first_seen = Some(iso.clone());
        }
        if p.stats.analysed_frames % 50 == 1 {
            p.stats.last_seen = Some(iso);
        }
        // Interface elements.
        for r in &obs.ui_regions {
            let kind = kind_word(&r.kind);
            match p.known_ui_elements.iter_mut().find(|e| same_class(&e.kind, kind) && e.norm.iou(&r.norm) > 0.5) {
                Some(e) => {
                    e.seen += 1;
                    e.norm = blend(e.norm, r.norm, 0.05);
                    if !e.corrected {
                        e.confidence = Confidence::new(e.confidence.value() * 0.95 + r.confidence.value() * 0.05);
                        if e.kind == "element" || (kind != "element" && e.kind != kind && e.seen % 20 == 0) {
                            e.kind = kind.to_string();
                        }
                    }
                    if r.appearance != 0 {
                        e.appearance = r.appearance;
                    }
                }
                None => {
                    let id = p.known_ui_elements.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                    p.known_ui_elements.push(KnownUiElement {
                        id,
                        norm: r.norm,
                        kind: kind.to_string(),
                        concept: None,
                        appearance: r.appearance,
                        seen: 1,
                        confidence: r.confidence,
                        corrected: false,
                        origin: "observed".into(),
                    });
                    if kind != "element" {
                        self.changes.push(format!("new {} at {}", kind.replace('_', " "), r.norm.place()));
                    }
                }
            }
        }
        if p.known_ui_elements.len() > 64 {
            p.known_ui_elements.sort_by(|a, b| {
                (b.corrected, b.concept.is_some(), b.seen).cmp(&(a.corrected, a.concept.is_some(), a.seen))
            });
            p.known_ui_elements.truncate(64);
        }
        // The HUD signature: the elements seen most, where they are and how they look.
        if p.stats.analysed_frames.is_multiple_of(25) {
            let mut best: Vec<&KnownUiElement> =
                p.known_ui_elements.iter().filter(|e| e.seen >= 20 && e.kind != "element").collect();
            best.sort_by_key(|e| std::cmp::Reverse(e.seen));
            let sig: Vec<HudMark> = best
                .iter()
                .take(12)
                .map(|e| HudMark { norm: e.norm, appearance: e.appearance, kind: e.kind.clone() })
                .collect();
            if sig.len() >= 3 && sig.len() != p.hud_signature.len() {
                self.changes.push(format!("HUD signature of {} elements", sig.len()));
            }
            if !sig.is_empty() {
                p.hud_signature = sig;
            }
        }
        // Recurring pictures.
        for r in obs.ui_regions.iter().filter(|r| matches!(r.kind, UiKind::Icon) && r.appearance != 0) {
            let room = p.visual_vocabulary.len() < 200;
            match p.visual_vocabulary.iter_mut().find(|w| hamming(w.appearance, r.appearance) <= 6) {
                Some(w) => w.seen += 1,
                None if room => p.visual_vocabulary.push(VisualWord {
                    appearance: r.appearance,
                    seen: 1,
                    first_seen_ms: now,
                    label: None,
                    size: (r.norm.w, r.norm.h),
                }),
                None => {}
            }
        }
        // The game's words.
        for t in obs.text.iter().filter(|t| t.fresh && t.confidence.at_least(0.5)) {
            for w in t.text.split(|c: char| !c.is_alphabetic() && c != '\'') {
                let w = w.trim_matches('\'').to_lowercase();
                if w.chars().count() < 3 || STOPWORDS.contains(&w.as_str()) {
                    continue;
                }
                let e = p.terminology.entry(w).or_default();
                e.count += 1;
                e.last_seen_ms = now;
            }
        }
        if p.terminology.len() > 1500 {
            let mut all: Vec<(String, u64)> = p.terminology.iter().map(|(k, v)| (k.clone(), v.count)).collect();
            all.sort_by_key(|t| std::cmp::Reverse(t.1));
            let keep: std::collections::HashSet<String> = all.into_iter().take(1200).map(|(k, _)| k).collect();
            p.terminology.retain(|k, _| keep.contains(k));
        }
        // Screens the game has.
        for e in &obs.events {
            if let ObservedEvent::SceneChanged { to, .. } = e {
                let name = to.word();
                match p.common_states.iter_mut().find(|s| s.name == name) {
                    Some(s) => {
                        let n: u64 =
                            s.description.split_whitespace().nth(1).and_then(|n| n.parse().ok()).unwrap_or(0) + 1;
                        s.description = format!("seen {n} times");
                    }
                    None => p.common_states.push(ProfileItem {
                        name: name.into(),
                        description: "seen 1 times".into(),
                        confidence: Confidence::new(0.6),
                        provenance: "observed".into(),
                    }),
                }
            }
        }
    }

    /// Writes concepts the state engine is now sure of onto their elements.
    pub fn learn_concepts(&mut self, p: &mut GameProfile, learned: &[ConceptLearned]) {
        for l in learned {
            let found = p
                .known_ui_elements
                .iter_mut()
                .filter(|e| same_class(&e.kind, &l.kind) && e.norm.iou(&l.norm) > 0.4)
                .max_by(|a, b| a.norm.iou(&l.norm).total_cmp(&b.norm.iou(&l.norm)));
            let e = match found {
                Some(e) => e,
                None => {
                    let id = p.known_ui_elements.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                    p.known_ui_elements.push(KnownUiElement {
                        id,
                        norm: l.norm,
                        kind: if l.kind == "bar" { "bar".into() } else { "text".into() },
                        concept: None,
                        appearance: 0,
                        seen: 1,
                        confidence: l.confidence,
                        corrected: false,
                        origin: "observed".into(),
                    });
                    p.known_ui_elements.last_mut().expect("just pushed")
                }
            };
            if e.corrected {
                continue;
            }
            if e.concept.as_deref() != Some(l.concept.as_str()) {
                self.changes.push(format!("{} at {} is {}", e.kind.replace('_', " "), e.norm.place(), l.concept));
            }
            e.concept = Some(l.concept.clone());
            e.confidence = Confidence::new(e.confidence.value().max(l.confidence.value()));
            let description = format!("{} at {} ({})", e.kind.replace('_', " "), e.norm.place(), l.evidence.join("; "));
            match p.resources.iter_mut().find(|r| r.name == l.concept) {
                Some(r) => {
                    r.description = description;
                    r.confidence = Confidence::new(r.confidence.value().max(l.confidence.value()));
                }
                None => p.resources.push(ProfileItem {
                    name: l.concept.clone(),
                    description,
                    confidence: l.confidence,
                    provenance: "observed".into(),
                }),
            }
        }
    }

    /// The player says the element at `norm` is `concept` (or nothing, when empty).
    pub fn correct(&mut self, p: &mut GameProfile, norm: NormRect, kind: &str, concept: &str) {
        let e = match p
            .known_ui_elements
            .iter_mut()
            .filter(|e| same_class(&e.kind, kind) && e.norm.iou(&norm) > 0.3)
            .max_by(|a, b| a.norm.iou(&norm).total_cmp(&b.norm.iou(&norm)))
        {
            Some(e) => e,
            None => {
                let id = p.known_ui_elements.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                p.known_ui_elements.push(KnownUiElement {
                    id,
                    norm,
                    kind: kind.into(),
                    concept: None,
                    appearance: 0,
                    seen: 0,
                    confidence: Confidence::CERTAIN,
                    corrected: true,
                    origin: "player".into(),
                });
                p.known_ui_elements.last_mut().expect("just pushed")
            }
        };
        e.concept = (!concept.is_empty()).then(|| concept.to_string());
        e.corrected = true;
        e.confidence = Confidence::CERTAIN;
        p.corrections.push(Correction {
            target: format!("{} at {}", kind, norm.place()),
            value: concept.into(),
            at: now_iso(),
        });
        self.changes.push(format!(
            "the player says the {} at {} is {}",
            kind,
            norm.place(),
            if concept.is_empty() { "nothing" } else { concept }
        ));
    }

    /// Starts a session: counted, and the time mark reset.
    pub fn start_session(&mut self, p: &mut GameProfile) {
        p.stats.sessions += 1;
        p.stats.last_seen = Some(now_iso());
        self.last_ms = None;
    }
}

fn blend(a: NormRect, b: NormRect, t: f32) -> NormRect {
    NormRect { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, w: a.w + (b.w - a.w) * t, h: a.h + (b.h - a.h) * t }
}

/// What the profile tells the state engine to expect.
pub fn hints(p: &GameProfile) -> Vec<ElementHint> {
    p.known_ui_elements
        .iter()
        .filter_map(|e| {
            e.concept.as_ref().map(|c| ElementHint {
                norm: e.norm,
                kind: if e.kind == "bar" { "bar".into() } else { e.kind.clone() },
                concept: c.clone(),
                confidence: e.confidence.value(),
                corrected: e.corrected,
                from: match e.origin.strip_prefix("plugin:") {
                    Some(p) => format!("the {p} plugin says so"),
                    None => "learned in an earlier session".into(),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use syrup_core::{Rect, UiRegion};

    use super::*;

    fn obs_with(regions: Vec<UiRegion>, t: u64) -> Observation {
        Observation { timestamp_ms: t, frame_size: (960, 540), ui_regions: regions, ..Default::default() }
    }

    fn bar(id: u32, x: i32) -> UiRegion {
        let rect = Rect::new(x, 500, 200, 20);
        UiRegion {
            id,
            rect,
            norm: rect.to_norm(960, 540),
            kind: UiKind::Bar { fill: 0.5, color: [200, 40, 40], vertical: false },
            stability: 1.0,
            confidence: Confidence::new(0.9),
            appearance: 42,
        }
    }

    #[test]
    fn elements_signature_and_concepts_are_learned() {
        let mut p = GameProfile::new("unknown-1", "Test");
        let mut l = ProfileLearner::new();
        for i in 0..60 {
            l.observe(&mut p, &obs_with(vec![bar(1, 50), bar(2, 300), bar(3, 600)], i * 200));
        }
        assert_eq!(p.known_ui_elements.len(), 3);
        assert_eq!(p.hud_signature.len(), 3);
        assert!(p.stats.observed_ms >= 11_000);
        let norm = Rect::new(50, 500, 200, 20).to_norm(960, 540);
        l.learn_concepts(
            &mut p,
            &[ConceptLearned {
                concept: "health".into(),
                source: "region:1".into(),
                kind: "bar".into(),
                norm,
                confidence: Confidence::new(0.8),
                evidence: vec!["red bar".into()],
            }],
        );
        assert_eq!(p.element_for_concept("health").map(|e| e.id), Some(1));
        // The player corrects it: final, and it wins over later inference.
        l.correct(&mut p, norm, "bar", "stamina");
        l.learn_concepts(
            &mut p,
            &[ConceptLearned {
                concept: "health".into(),
                source: "region:1".into(),
                kind: "bar".into(),
                norm,
                confidence: Confidence::new(0.9),
                evidence: vec![],
            }],
        );
        assert!(p.element_for_concept("health").is_none());
        let h = hints(&p);
        assert!(h.iter().any(|h| h.concept == "stamina" && h.corrected));
        assert!(!l.take_changes().is_empty());
    }
}
