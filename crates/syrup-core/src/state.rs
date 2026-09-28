//! What Syrup believes about the game right now: concepts discovered from
//! the observation stream (each with its evidence), how busy the game is,
//! and what just changed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::confidence::{Confidence, Reliability};
use crate::observation::SceneKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trend {
    Rising,
    Falling,
    Steady,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptUnit {
    /// 0..1, e.g. how full a bar is.
    #[default]
    Fraction,
    /// A count: score, gold, ammo, level.
    Count,
    /// A time, in seconds.
    Seconds,
    /// Just text: a location name, an objective.
    Text,
}

/// One concept a game turned out to have ("health", "score", "gold"...), its
/// current value and why Syrup believes this element is that concept.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ConceptValue {
    pub name: String,
    pub value: Option<f64>,
    /// The largest value seen or read ("87/100" gives 100), when known.
    pub max: Option<f64>,
    pub text: Option<String>,
    pub unit: ConceptUnit,
    /// The element it is read from, e.g. `region:3`.
    pub source: String,
    /// How sure Syrup is that the element *is* this concept.
    pub confidence: Confidence,
    /// How the value itself was obtained.
    pub reliability: Reliability,
    pub trend: Trend,
    pub evidence: Vec<String>,
    pub updated_ms: u64,
}

impl ConceptValue {
    /// The value as a fraction of its maximum, when that makes sense.
    pub fn fraction(&self) -> Option<f64> {
        match (self.unit, self.value, self.max) {
            (ConceptUnit::Fraction, Some(v), _) => Some(v),
            (_, Some(v), Some(m)) if m > 0.0 => Some(v / m),
            _ => None,
        }
    }
}

/// How busy the game is.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Activity {
    /// 0 (calm) to 1 (everything is moving and changing).
    pub intensity: f32,
    /// Fraction of the frame covered by motion.
    pub motion: f32,
    /// How long nothing much has happened, in milliseconds.
    pub idle_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    ResourceFell,
    ResourceRose,
    ResourceLow,
    ResourceEmpty,
    ResourceRecovered,
    CounterIncreased,
    CounterDecreased,
    TextChanged,
    ElementAppeared,
    ElementDisappeared,
    SceneChanged,
    PlayerDied,
    Victory,
    RoundStarted,
    LevelUp,
    AreaRevisited,
    RepeatedFailure,
    Idle,
    ObjectiveChanged,
}

impl TransitionKind {
    pub fn word(self) -> &'static str {
        match self {
            TransitionKind::ResourceFell => "fell",
            TransitionKind::ResourceRose => "rose",
            TransitionKind::ResourceLow => "low",
            TransitionKind::ResourceEmpty => "empty",
            TransitionKind::ResourceRecovered => "recovered",
            TransitionKind::CounterIncreased => "went up",
            TransitionKind::CounterDecreased => "went down",
            TransitionKind::TextChanged => "text changed",
            TransitionKind::ElementAppeared => "appeared",
            TransitionKind::ElementDisappeared => "disappeared",
            TransitionKind::SceneChanged => "scene changed",
            TransitionKind::PlayerDied => "died",
            TransitionKind::Victory => "won",
            TransitionKind::RoundStarted => "round started",
            TransitionKind::LevelUp => "level up",
            TransitionKind::AreaRevisited => "back in a place seen before",
            TransitionKind::RepeatedFailure => "repeated failure",
            TransitionKind::Idle => "idle",
            TransitionKind::ObjectiveChanged => "objective changed",
        }
    }
}

/// A meaningful change of state. Syrup stores these, not raw frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub ts_ms: u64,
    pub kind: TransitionKind,
    /// What changed: a concept name ("health"), an element ("region:3") or the scene.
    pub subject: String,
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub detail: String,
    pub confidence: Confidence,
}

/// A belief about what is going on, with its evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub statement: String,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
}

/// The game as Syrup understands it at one moment.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GameState {
    pub timestamp_ms: u64,
    pub scene: SceneKind,
    pub concepts: BTreeMap<String, ConceptValue>,
    pub activity: Activity,
    /// The last few transitions, newest last.
    pub recent: Vec<Transition>,
    pub hypotheses: Vec<Hypothesis>,
}

impl GameState {
    pub fn concept(&self, name: &str) -> Option<&ConceptValue> {
        self.concepts.get(name)
    }

    /// The fraction of a concept (health, energy...) if it is known well enough.
    pub fn fraction_of(&self, name: &str, min_confidence: f32) -> Option<f64> {
        self.concepts
            .get(name)
            .filter(|c| c.confidence.at_least(min_confidence))
            .and_then(|c| c.fraction())
    }

    pub fn happened(
        &self,
        kind: TransitionKind,
        since_ms: u64,
    ) -> impl Iterator<Item = &Transition> {
        self.recent
            .iter()
            .filter(move |t| t.kind == kind && t.ts_ms >= since_ms)
    }
}
