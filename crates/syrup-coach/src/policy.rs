//! What is worth interrupting the player for, and what would spoil the game.

use serde::{Deserialize, Serialize};
use syrup_core::knowledge::{FactKind, SpoilerLevel};
use syrup_core::{Advice, SceneKind, Urgency};
use syrup_player::GameModel;

/// How much knowledge-based advice may give away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoilerPolicy {
    /// Only what Syrup saw for itself: no looked-up facts at all.
    None,
    /// Nudges, never answers, and nothing about what is ahead.
    HintsOnly,
    /// Facts, but nothing that gives away story or surprises.
    #[default]
    Normal,
    /// Everything known.
    FullInformation,
}

impl SpoilerPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "none" | "off" => Some(SpoilerPolicy::None),
            "hints" | "hints_only" => Some(SpoilerPolicy::HintsOnly),
            "normal" => Some(SpoilerPolicy::Normal),
            "full" | "full_information" | "all" => Some(SpoilerPolicy::FullInformation),
            _ => None,
        }
    }

    /// The most a fact may give away under this policy.
    pub fn max_spoiler(self) -> SpoilerLevel {
        match self {
            SpoilerPolicy::None | SpoilerPolicy::HintsOnly => SpoilerLevel::None,
            SpoilerPolicy::Normal => SpoilerLevel::Mild,
            SpoilerPolicy::FullInformation => SpoilerLevel::Major,
        }
    }

    /// Whether this advice may be given at all.
    pub fn allows(self, a: &Advice) -> Result<(), String> {
        let looked_up = a.rests_on.iter().any(|k| *k != FactKind::Inference);
        match self {
            SpoilerPolicy::None if looked_up => Err("spoilers are off: nothing looked up".into()),
            _ if a.spoiler > self.max_spoiler() => {
                Err(format!("would give away too much ({:?})", a.spoiler).to_lowercase())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptionPolicy {
    /// Quiet time between two lines (critical ones excepted).
    pub min_gap_ms: u64,
    /// Quiet time between two critical lines.
    pub critical_gap_ms: u64,
    /// The same topic is not repeated within this.
    pub topic_cooldown_ms: u64,
    /// Non-critical lines per minute, at most.
    pub per_minute: usize,
    /// Above this intensity the player is busy.
    pub busy: f32,
}

impl Default for InterruptionPolicy {
    fn default() -> Self {
        InterruptionPolicy {
            min_gap_ms: 8000,
            critical_gap_ms: 3000,
            topic_cooldown_ms: 45_000,
            per_minute: 4,
            busy: 0.45,
        }
    }
}

/// Everything the decision looks at.
pub struct Moment<'a> {
    pub now_ms: u64,
    pub intensity: f32,
    pub scene: SceneKind,
    pub last_spoken_ms: Option<u64>,
    /// (topic, when, urgency) of what was said lately.
    pub spoken: &'a [(String, u64, Urgency)],
    pub player: Option<&'a GameModel>,
}

pub fn urgency_value(u: Urgency) -> f32 {
    match u {
        Urgency::Critical => 1.0,
        Urgency::Important => 0.7,
        Urgency::Opportunistic => 0.45,
        Urgency::Educational => 0.3,
    }
}

impl InterruptionPolicy {
    /// How much saying this now is worth (0 when it must not be said), or why not.
    pub fn value(&self, a: &Advice, m: &Moment) -> Result<f32, String> {
        if a.expires_ms <= m.now_ms {
            return Err("too late: the moment passed".into());
        }
        if let Some(p) = m.player
            && p.muted_topics.contains(&a.topic)
        {
            return Err("the player muted this".into());
        }
        if let Some((_, t, _)) = m.spoken.iter().rev().find(|(topic, _, _)| *topic == a.topic)
            && m.now_ms.saturating_sub(*t) < self.topic_cooldown_ms
        {
            return Err(format!("said {}s ago", m.now_ms.saturating_sub(*t) / 1000));
        }
        let critical = a.urgency == Urgency::Critical;
        match m.scene {
            SceneKind::Cutscene | SceneKind::Dialogue if !critical => {
                return Err("not over a cutscene or dialogue".into());
            }
            SceneKind::Loading | SceneKind::Menu
                if matches!(a.urgency, Urgency::Important) && a.kind != syrup_core::AdviceKind::Status =>
            {
                return Err("nothing to act on in a menu".into());
            }
            _ => {}
        }
        if let Some(last) = m.last_spoken_ms {
            let gap = m.now_ms.saturating_sub(last);
            if critical && gap < self.critical_gap_ms {
                return Err("just spoke".into());
            }
            if !critical && gap < self.min_gap_ms {
                return Err(format!("spoke {}s ago", gap / 1000));
            }
        }
        let recent =
            m.spoken.iter().filter(|(_, t, u)| m.now_ms.saturating_sub(*t) < 60_000 && *u != Urgency::Critical).count();
        if !critical && recent >= self.per_minute {
            return Err("enough talking this minute".into());
        }
        let weight = m
            .player
            .map(|p| p.weight(&format!("kind:{:?}", a.kind)) * p.weight(&format!("topic:{}", a.topic)))
            .unwrap_or(1.0);
        let value = urgency_value(a.urgency) * a.confidence.value() * weight;
        if m.intensity > self.busy && !critical && (a.urgency != Urgency::Important || value < 0.45) {
            return Err("the player is busy".into());
        }
        if value < 0.12 {
            return Err(format!("not worth it (value {value:.2})"));
        }
        Ok(value)
    }
}
