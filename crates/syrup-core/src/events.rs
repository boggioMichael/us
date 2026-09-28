//! Everything that happens, as events on one bus.
//!
//! Modules publish; the devtools page, the session timeline and anything
//! else read. The bus keeps a numbered ring of recent events so a reader can
//! ask for "everything after #1234", and hands live copies to subscribers.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::advice::{Advice, Feedback};
use crate::confidence::Confidence;
use crate::identity::GameIdentity;
use crate::observation::SceneKind;
use crate::state::Transition;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A heartbeat from capture: frames seen and analysed in the last second.
    FrameCaptured {
        ts_ms: u64,
        frame: u64,
        width: u32,
        height: u32,
        captured_per_s: f32,
        analysed_per_s: f32,
    },
    SceneChanged {
        ts_ms: u64,
        from: SceneKind,
        to: SceneKind,
    },
    GameIdentified {
        ts_ms: u64,
        identity: GameIdentity,
    },
    /// Not sure yet: the best guess so far, if any.
    GameUncertain {
        ts_ms: u64,
        best: Option<GameIdentity>,
    },
    UiElementDiscovered {
        ts_ms: u64,
        region: u32,
        kind: String,
        place: String,
    },
    ConceptLearned {
        ts_ms: u64,
        concept: String,
        source: String,
        confidence: Confidence,
        evidence: Vec<String>,
    },
    StateChanged {
        transition: Transition,
    },
    PlayerDied {
        ts_ms: u64,
        context: String,
    },
    ObjectiveChanged {
        ts_ms: u64,
        text: String,
    },
    ResearchRequested {
        ts_ms: u64,
        game_id: String,
        question: String,
        reason: String,
    },
    KnowledgeUpdated {
        ts_ms: u64,
        game_id: String,
        question: String,
        facts: usize,
        sources: usize,
    },
    ResearchFailed {
        ts_ms: u64,
        game_id: String,
        question: String,
        reason: String,
    },
    AdviceGenerated {
        advice: Advice,
    },
    AdviceShown {
        ts_ms: u64,
        advice_id: u64,
        text: String,
    },
    AdviceSuppressed {
        ts_ms: u64,
        advice_id: u64,
        topic: String,
        reason: String,
    },
    FeedbackReceived {
        feedback: Feedback,
    },
    ProfileUpdated {
        ts_ms: u64,
        game_id: String,
        reason: String,
    },
    PluginActivated {
        ts_ms: u64,
        plugin: String,
        game_id: String,
    },
    Note {
        ts_ms: u64,
        message: String,
    },
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::FrameCaptured { .. } => "frame_captured",
            Event::SceneChanged { .. } => "scene_changed",
            Event::GameIdentified { .. } => "game_identified",
            Event::GameUncertain { .. } => "game_uncertain",
            Event::UiElementDiscovered { .. } => "ui_element_discovered",
            Event::ConceptLearned { .. } => "concept_learned",
            Event::StateChanged { .. } => "state_changed",
            Event::PlayerDied { .. } => "player_died",
            Event::ObjectiveChanged { .. } => "objective_changed",
            Event::ResearchRequested { .. } => "research_requested",
            Event::KnowledgeUpdated { .. } => "knowledge_updated",
            Event::ResearchFailed { .. } => "research_failed",
            Event::AdviceGenerated { .. } => "advice_generated",
            Event::AdviceShown { .. } => "advice_shown",
            Event::AdviceSuppressed { .. } => "advice_suppressed",
            Event::FeedbackReceived { .. } => "feedback_received",
            Event::ProfileUpdated { .. } => "profile_updated",
            Event::PluginActivated { .. } => "plugin_activated",
            Event::Note { .. } => "note",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub seq: u64,
    pub event: Event,
}

struct Inner {
    next: u64,
    ring: VecDeque<EventRecord>,
    capacity: usize,
    subscribers: Vec<Sender<EventRecord>>,
}

/// A cheap-to-clone handle to the bus.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<Inner>>,
}

impl Default for EventBus {
    fn default() -> Self {
        EventBus::new(4096)
    }
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        EventBus {
            inner: Arc::new(Mutex::new(Inner {
                next: 1,
                ring: VecDeque::with_capacity(capacity.min(4096)),
                capacity: capacity.max(1),
                subscribers: Vec::new(),
            })),
        }
    }

    /// Publish an event; returns its sequence number.
    pub fn publish(&self, event: Event) -> u64 {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let seq = inner.next;
        inner.next += 1;
        let record = EventRecord { seq, event };
        inner.subscribers.retain(|s| s.send(record.clone()).is_ok());
        if inner.ring.len() == inner.capacity {
            inner.ring.pop_front();
        }
        inner.ring.push_back(record);
        seq
    }

    /// Every event still in the ring with a sequence number above `seq`.
    pub fn since(&self, seq: u64) -> Vec<EventRecord> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.ring.iter().filter(|r| r.seq > seq).cloned().collect()
    }

    /// The last `n` events.
    pub fn last(&self, n: usize) -> Vec<EventRecord> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.ring.iter().rev().take(n).rev().cloned().collect()
    }

    /// Live copies of every event from now on.
    pub fn subscribe(&self) -> Receiver<EventRecord> {
        let (tx, rx) = channel();
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .subscribers
            .push(tx);
        rx
    }

    pub fn latest_seq(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).next - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_newest_and_numbers_everything() {
        let bus = EventBus::new(3);
        let rx = bus.subscribe();
        for i in 0..5 {
            bus.publish(Event::Note {
                ts_ms: i,
                message: format!("{i}"),
            });
        }
        let kept: Vec<u64> = bus.since(0).iter().map(|r| r.seq).collect();
        assert_eq!(kept, vec![3, 4, 5]);
        assert_eq!(bus.since(4).len(), 1);
        assert_eq!(rx.try_iter().count(), 5);
        assert_eq!(bus.latest_seq(), 5);
        let json = serde_json::to_string(&bus.last(1)[0].event).unwrap();
        assert!(json.contains("\"type\":\"note\""), "{json}");
    }
}
