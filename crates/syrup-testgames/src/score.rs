//! How well Syrup's view of a test game matches the game's truth.
//!
//! Fed once per analysed frame with what Syrup believed (the scene, and each
//! concept's value as a fraction); reports how often the scene was right,
//! and for each concept on screen how often Syrup knew it and how far off its
//! value was. Event counts (deaths, wins, level-ups) come from the log.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use syrup_core::SceneKind;

use crate::{Truth, TruthEventKind, TruthLog};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConceptScore {
    /// Analysed frames the element was on screen with a value.
    pub shown: u32,
    /// Of those, frames Syrup had the concept.
    pub known: u32,
    /// Sum of |Syrup's fraction − the true fraction| over `compared` frames.
    pub error_sum: f64,
    pub compared: u32,
}

impl ConceptScore {
    /// The share of frames Syrup knew the concept while it was on screen.
    pub fn coverage(&self) -> f64 {
        self.known as f64 / self.shown.max(1) as f64
    }

    pub fn mean_error(&self) -> Option<f64> {
        (self.compared > 0).then(|| self.error_sum / self.compared as f64)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub frames: u32,
    pub scene_right: u32,
    pub concepts: BTreeMap<String, ConceptScore>,
}

/// What Syrup believed about a concept: not known, known without a fraction, or its fraction.
pub type Belief = Option<Option<f64>>;

impl Score {
    /// One analysed frame: the truth, Syrup's scene, and Syrup's belief per concept.
    pub fn observe(&mut self, truth: &Truth, scene: SceneKind, belief: impl Fn(&str) -> Belief) {
        self.frames += 1;
        if scene == truth.scene {
            self.scene_right += 1;
        }
        for e in &truth.elements {
            let Some(want) = e.fraction() else { continue };
            let s = self.concepts.entry(e.concept.clone()).or_default();
            s.shown += 1;
            if let Some(got) = belief(&e.concept) {
                s.known += 1;
                if let Some(got) = got {
                    s.error_sum += (got - want).abs();
                    s.compared += 1;
                }
            }
        }
    }

    pub fn scene_accuracy(&self) -> f64 {
        self.scene_right as f64 / self.frames.max(1) as f64
    }
}

/// Deaths (and lost matches), wins and level-ups that really happened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Happened {
    pub defeats: usize,
    pub wins: usize,
    pub level_ups: usize,
}

impl Happened {
    pub fn from(log: &TruthLog) -> Happened {
        let count = |f: &dyn Fn(&TruthEventKind) -> bool| log.events.iter().filter(|e| f(&e.kind)).count();
        Happened {
            defeats: count(&|k| matches!(k, TruthEventKind::Died | TruthEventKind::Defeat)),
            wins: count(&|k| matches!(k, TruthEventKind::Victory)),
            level_ups: count(&|k| matches!(k, TruthEventKind::LevelUp { .. })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TruthElement;
    use syrup_core::Rect;

    #[test]
    fn scores_add_up() {
        let bar = TruthElement {
            name: "hp".into(),
            concept: "health".into(),
            kind: "bar".into(),
            rect: Rect::new(0, 0, 10, 2),
            value: Some(50.0),
            max: Some(100.0),
            text: None,
            color: None,
        };
        let truth = Truth { t_ms: 0, scene: SceneKind::Gameplay, elements: vec![bar], player: None };
        let mut s = Score::default();
        s.observe(&truth, SceneKind::Gameplay, |c| (c == "health").then_some(Some(0.6)));
        s.observe(&truth, SceneKind::Menu, |_| None);
        assert_eq!(s.scene_accuracy(), 0.5);
        let h = &s.concepts["health"];
        assert_eq!((h.shown, h.known, h.coverage()), (2, 1, 0.5));
        assert!((h.mean_error().unwrap() - 0.1).abs() < 1e-9);
    }
}
