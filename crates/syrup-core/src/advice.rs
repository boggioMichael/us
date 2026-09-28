//! Advice Syrup may give, and what the player says back.

use serde::{Deserialize, Serialize};

use crate::confidence::Confidence;
use crate::knowledge::{FactKind, SpoilerLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AdviceKind {
    ImmediateWarning,
    Tactical,
    Strategic,
    Build,
    Economy,
    Route,
    ObjectiveReminder,
    MechanicalCorrection,
    Learning,
    PostGame,
    /// Syrup saying what it is doing ("I think this is Elden Ring", "checking...").
    Status,
}

/// How much it matters right now. Ordered: `Critical` is the greatest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Educational,
    Opportunistic,
    Important,
    Critical,
}

impl Urgency {
    pub fn word(self) -> &'static str {
        match self {
            Urgency::Critical => "critical",
            Urgency::Important => "important",
            Urgency::Opportunistic => "opportunistic",
            Urgency::Educational => "educational",
        }
    }
}

/// How Syrup looks while saying it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expression {
    #[default]
    Neutral,
    Thinking,
    Excited,
    Warning,
    Confused,
    Proud,
    Researching,
    Surprised,
}

impl Expression {
    pub const ALL: [Expression; 8] = [
        Expression::Neutral,
        Expression::Thinking,
        Expression::Excited,
        Expression::Warning,
        Expression::Confused,
        Expression::Proud,
        Expression::Researching,
        Expression::Surprised,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Expression::Neutral => "neutral",
            Expression::Thinking => "thinking",
            Expression::Excited => "excited",
            Expression::Warning => "warning",
            Expression::Confused => "confused",
            Expression::Proud => "proud",
            Expression::Researching => "researching",
            Expression::Surprised => "surprised",
        }
    }
}

/// One piece of advice, ready to be judged by the interruption policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advice {
    pub id: u64,
    /// What it is about, for cooldowns and muting: `low:health`, `repeat-death`...
    pub topic: String,
    pub kind: AdviceKind,
    pub urgency: Urgency,
    /// What Syrup says: short.
    pub text: String,
    /// The evidence behind it, for "why?".
    pub why: Vec<String>,
    pub confidence: Confidence,
    pub spoiler: SpoilerLevel,
    /// The kinds of knowledge it rests on (empty for pure observation).
    pub rests_on: Vec<FactKind>,
    pub expression: Expression,
    /// Which rule or plugin made it.
    pub origin: String,
    pub created_ms: u64,
    /// Not worth saying after this time.
    pub expires_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    /// 👍 useful.
    Useful,
    /// 👎 wrong.
    Wrong,
    /// ❓ explain.
    Explain,
    /// 🔇 stop suggesting this.
    StopSuggesting,
    /// Dismissed without judging it.
    Ignore,
    /// "Research this."
    Research,
}

impl FeedbackKind {
    pub fn symbol(self) -> &'static str {
        match self {
            FeedbackKind::Useful => "👍",
            FeedbackKind::Wrong => "👎",
            FeedbackKind::Explain => "❓",
            FeedbackKind::StopSuggesting => "🔇",
            FeedbackKind::Ignore => "✕",
            FeedbackKind::Research => "🔎",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feedback {
    pub advice_id: u64,
    pub topic: String,
    pub kind: FeedbackKind,
    pub at_ms: u64,
}
