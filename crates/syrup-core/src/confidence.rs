//! How sure something is, and how it came to be believed.
//!
//! Ported from Maplesyrup's `vision::types`: a clamped confidence that
//! combines like independent evidence, and a reliability grade that says
//! what kind of evidence it is.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A confidence in [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Confidence(f32);

impl Confidence {
    pub const NONE: Confidence = Confidence(0.0);
    pub const CERTAIN: Confidence = Confidence(1.0);

    pub fn new(value: f32) -> Self {
        if value.is_nan() { Confidence(0.0) } else { Confidence(value.clamp(0.0, 1.0)) }
    }

    pub fn value(self) -> f32 {
        self.0
    }

    /// Two independent pieces of evidence for the same thing (probabilistic OR).
    pub fn combine(self, other: Confidence) -> Confidence {
        Confidence::new(self.0 + other.0 - self.0 * other.0)
    }

    /// Evidence from several sources, combined.
    pub fn combine_all(items: impl IntoIterator<Item = Confidence>) -> Confidence {
        items.into_iter().fold(Confidence::NONE, Confidence::combine)
    }

    /// Weakened, e.g. because the observation is getting old.
    pub fn decay(self, factor: f32) -> Confidence {
        Confidence::new(self.0 * factor.clamp(0.0, 1.0))
    }

    /// The weaker of the two: a chain is as strong as its weakest link.
    pub fn and(self, other: Confidence) -> Confidence {
        Confidence::new(self.0 * other.0)
    }

    pub fn at_least(self, threshold: f32) -> bool {
        self.0 >= threshold
    }

    /// A word for people: "sure", "fairly sure", "guessing", "no idea".
    pub fn word(self) -> &'static str {
        match self.0 {
            v if v >= 0.85 => "sure",
            v if v >= 0.6 => "fairly sure",
            v if v >= 0.3 => "guessing",
            _ => "no idea",
        }
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0}%", self.0 * 100.0)
    }
}

/// What kind of evidence a belief rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reliability {
    /// Independent signals agree (a bar and the number printed on it).
    Corroborated,
    /// One geometric or colour signal, nothing to confirm it.
    #[default]
    Heuristic,
    /// Carried over from earlier frames; nothing seen this frame.
    Predicted,
    /// The detection failed; see its failure reason.
    Unreliable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_adds_up_but_never_past_certain() {
        let a = Confidence::new(0.5);
        assert!((a.combine(a).value() - 0.75).abs() < 1e-6);
        assert_eq!(Confidence::new(3.0), Confidence::CERTAIN);
        assert_eq!(Confidence::new(f32::NAN), Confidence::NONE);
        assert!((Confidence::combine_all([a, a, a]).value() - 0.875).abs() < 1e-6);
        assert_eq!(Confidence::new(0.9).word(), "sure");
        assert_eq!(Confidence::new(0.1).word(), "no idea");
    }
}
