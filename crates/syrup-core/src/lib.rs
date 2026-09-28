//! The shared vocabulary of Syrup Universal.
//!
//! Every other crate speaks in these types: geometry, confidence and the
//! detection contract (from Maplesyrup), the observation IR that perception
//! fills, the game state and profiles built from it, knowledge with
//! provenance, advice and feedback, and the events that tie the modules
//! together. Nothing here knows any particular game.

pub mod advice;
pub mod confidence;
pub mod detection;
pub mod events;
pub mod frame;
pub mod geometry;
pub mod identity;
pub mod knowledge;
pub mod observation;
pub mod paths;
pub mod profile;
pub mod state;
pub mod util;

pub use advice::{Advice, AdviceKind, Expression, Feedback, FeedbackKind, Urgency};
pub use confidence::{Confidence, Reliability};
pub use detection::Detection;
pub use events::{Event, EventBus, EventRecord};
pub use frame::{Frame, SourceInfo, SourceKind};
pub use geometry::{NormRect, Rect};
pub use identity::{Evidence, GameIdentity, HudMark, IdentityCues};
pub use observation::{Observation, SceneKind, UiKind, UiRegion};
pub use paths::DataDir;
pub use profile::{GameProfile, Hat, KnownUiElement, VisualIdentity};
pub use state::{ConceptValue, GameState, Transition, TransitionKind, Trend};
