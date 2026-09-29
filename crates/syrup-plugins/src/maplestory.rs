//! MapleStory, carried over from Maplesyrup.
//!
//! The data (interface priors, dialog words, runes and portals, early
//! monsters, training heuristics) is `plugins/maplestory/plugin.json`. The
//! code adds what data cannot say: reading the boss map's death count and
//! timer, and warning when either runs low.

use std::sync::OnceLock;

use regex::Regex;
use syrup_core::advice::{AdviceKind, Expression, Urgency};
use syrup_core::knowledge::{Fact, KnowledgeNode, KnowledgeSource, SpoilerLevel};
use syrup_core::state::ConceptUnit;
use syrup_core::{
    Advice, ConceptValue, Confidence, GameIdentity, GameState, IdentityCues, Observation, Reliability, SceneKind,
    VisualIdentity,
};
use syrup_state::ElementHint;

use crate::{DataPlugin, GamePlugin, PluginContext};

const SPEC: &str = include_str!("../../../plugins/maplestory/plugin.json");

pub struct MapleStory {
    data: DataPlugin,
    death_count: Option<u32>,
    warned_deaths: Option<u32>,
    warned_timer: bool,
    next_id: u64,
}

impl MapleStory {
    pub fn new() -> Self {
        MapleStory {
            data: DataPlugin::from_json(SPEC).expect("the MapleStory plugin's JSON is valid"),
            death_count: None,
            warned_deaths: None,
            warned_timer: false,
            next_id: 2_000_000,
        }
    }
}

impl Default for MapleStory {
    fn default() -> Self {
        MapleStory::new()
    }
}

/// "DEATH COUNT 05" (boss maps allow a fixed number of deaths).
pub fn death_count(text: &str) -> Option<u32> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)death\s*count\D{0,6}(\d{1,2})").expect("valid pattern"));
    re.captures(text).and_then(|c| c[1].parse().ok())
}

/// "Time Left: 29 min 32 sec" on boss maps, in seconds.
pub fn time_left(text: &str) -> Option<u32> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)(\d{1,2})\s*min\s*(\d{1,2})\s*sec").expect("valid pattern"));
    re.captures(text).map(|c| c[1].parse::<u32>().unwrap_or(0) * 60 + c[2].parse::<u32>().unwrap_or(0))
}

impl GamePlugin for MapleStory {
    fn id(&self) -> &str {
        "maplestory"
    }

    fn name(&self) -> &str {
        "MapleStory"
    }

    fn detect(&self, cues: &IdentityCues, identity: &GameIdentity) -> Option<Confidence> {
        self.data.detect(cues, identity)
    }

    fn extract_state(&mut self, obs: &Observation, state: &mut GameState) {
        let text = obs.all_text_lower();
        if let Some(n) = death_count(&text) {
            self.death_count = Some(n);
            state.concepts.insert(
                "deaths_left".into(),
                ConceptValue {
                    name: "deaths_left".into(),
                    value: Some(n as f64),
                    unit: ConceptUnit::Count,
                    source: "plugin:maplestory".into(),
                    confidence: Confidence::new(0.85),
                    reliability: Reliability::Heuristic,
                    evidence: vec!["\"DEATH COUNT\" on screen".into()],
                    updated_ms: obs.timestamp_ms,
                    ..Default::default()
                },
            );
        }
        if let Some(secs) = time_left(&text) {
            state.concepts.insert(
                "boss_timer".into(),
                ConceptValue {
                    name: "boss_timer".into(),
                    value: Some(secs as f64),
                    unit: ConceptUnit::Seconds,
                    source: "plugin:maplestory".into(),
                    confidence: Confidence::new(0.85),
                    reliability: Reliability::Heuristic,
                    evidence: vec!["\"Time Left\" on screen".into()],
                    updated_ms: obs.timestamp_ms,
                    ..Default::default()
                },
            );
        }
    }

    fn known_regions(&self) -> Vec<ElementHint> {
        self.data.known_regions()
    }

    fn known_entities(&self) -> Vec<KnowledgeNode> {
        self.data.known_entities()
    }

    fn seed_facts(&self) -> Vec<Fact> {
        self.data.seed_facts()
    }

    fn knowledge_sources(&self) -> Vec<KnowledgeSource> {
        self.data.knowledge_sources()
    }

    fn visual_theme(&self) -> Option<VisualIdentity> {
        self.data.visual_theme()
    }

    fn scene_words(&self) -> Vec<(SceneKind, String)> {
        self.data.scene_words()
    }

    fn genres(&self) -> Vec<String> {
        self.data.genres()
    }

    fn absent_concepts(&self) -> Vec<String> {
        self.data.absent_concepts()
    }

    fn advice(&mut self, ctx: &PluginContext) -> Vec<Advice> {
        let mut out = self.data.advice(ctx);
        let make = |id: &mut u64, topic: &str, text: String, why: String, urgency: Urgency| {
            *id += 1;
            Advice {
                id: *id,
                topic: topic.into(),
                kind: AdviceKind::ImmediateWarning,
                urgency,
                text,
                why: vec![why],
                confidence: Confidence::new(0.85),
                spoiler: SpoilerLevel::None,
                rests_on: Vec::new(),
                expression: Expression::Warning,
                origin: "plugin:maplestory".into(),
                created_ms: ctx.now_ms,
                expires_ms: ctx.now_ms + 6000,
            }
        };
        if let Some(n) = ctx.state.concept("deaths_left").and_then(|c| c.value).map(|v| v as u32)
            && n <= 2
            && self.warned_deaths != Some(n)
        {
            self.warned_deaths = Some(n);
            let text = if n == 1 {
                "Last life in this fight. Play it safe.".to_string()
            } else {
                format!("Only {n} deaths left. Careful.")
            };
            out.push(make(
                &mut self.next_id,
                "maplestory:death-count",
                text,
                format!("DEATH COUNT {n:02}"),
                Urgency::Important,
            ));
        }
        if let Some(secs) = ctx.state.concept("boss_timer").and_then(|c| c.value) {
            if secs <= 180.0 && !self.warned_timer {
                self.warned_timer = true;
                out.push(make(
                    &mut self.next_id,
                    "maplestory:boss-timer",
                    "Three minutes left on the boss timer.".into(),
                    format!("Time Left {:.0}s", secs),
                    Urgency::Important,
                ));
            } else if secs > 240.0 {
                self.warned_timer = false;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maplesyrup_knowledge_comes_along() {
        let p = MapleStory::new();
        assert_eq!(p.visual_theme().unwrap().hat, syrup_core::Hat::SyrupCap);
        assert!(p.known_regions().iter().any(|h| h.concept == "health" && h.kind == "bar"));
        assert!(p.seed_facts().iter().any(|f| f.claim.contains("rune")));
        assert!(p.known_entities().iter().any(|e| e.name == "Wild Boar"));
        assert!(p.scene_words().iter().any(|(k, w)| *k == SceneKind::Defeat && w == "return to town"));
        assert!(p.knowledge_sources().iter().any(|s| s.locator.contains("maplestory.fandom.com")));
        assert!(p.absent_concepts().contains(&"stamina".to_string()));
    }

    #[test]
    fn boss_map_counters_are_read() {
        assert_eq!(death_count("DEATH COUNT 05"), Some(5));
        assert_eq!(death_count("death count: 1"), Some(1));
        assert_eq!(time_left("Time Left: 29 min 32 sec"), Some(29 * 60 + 32));
        let mut p = MapleStory::new();
        let obs = Observation {
            timestamp_ms: 1000,
            text: vec![syrup_core::observation::TextItem {
                text: "DEATH COUNT 02".into(),
                rect: syrup_core::Rect::new(0, 0, 10, 10),
                region: None,
                confidence: Confidence::new(0.9),
                engine: "t".into(),
                fresh: true,
            }],
            ..Default::default()
        };
        let mut state = GameState::default();
        p.extract_state(&obs, &mut state);
        assert_eq!(state.concept("deaths_left").and_then(|c| c.value), Some(2.0));
        let id = GameIdentity {
            game_id: "maplestory".into(),
            title: "MapleStory".into(),
            confidence: Confidence::new(0.9),
            version: None,
            platform: None,
            evidence: vec![],
            confirmed: false,
        };
        let ctx = PluginContext { now_ms: 1000, identity: &id, observation: &obs, state: &state, profile: None };
        let advice = p.advice(&ctx);
        assert!(advice.iter().any(|a| a.text.contains("2 deaths left")), "{advice:?}");
        assert!(p.advice(&ctx).is_empty(), "said once");
    }
}
