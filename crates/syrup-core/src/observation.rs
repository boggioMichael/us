//! The universal intermediate representation: what one analysed frame
//! showed, with no game-specific meaning attached yet.
//!
//! Perception fills an [`Observation`]; the temporal tracker, the game-state
//! engine, plugins and the recogniser read it. Nothing here says "health" or
//! "minimap of MapleStory": a region is a bar with a fill and a colour, a
//! square that keeps a little inner motion is a minimap-like panel, and what
//! they *mean* is decided later, with evidence.

use serde::{Deserialize, Serialize};

use crate::confidence::Confidence;
use crate::geometry::{NormRect, Rect};

/// What kind of screen this is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum SceneKind {
    #[default]
    Unknown,
    /// The game is being played.
    Gameplay,
    /// A menu: little motion, mostly text.
    Menu,
    /// A loading screen: mostly still, dark or plain, maybe a spinner or bar.
    Loading,
    /// A dialogue or message box over the game.
    Dialogue,
    /// Motion without interface: a cutscene.
    Cutscene,
    /// A death, defeat or game-over screen.
    Defeat,
    /// A victory or level-cleared screen.
    Victory,
    /// A big map or inventory screen over the game.
    Overlay,
}

impl SceneKind {
    pub fn word(self) -> &'static str {
        match self {
            SceneKind::Unknown => "unknown",
            SceneKind::Gameplay => "gameplay",
            SceneKind::Menu => "menu",
            SceneKind::Loading => "loading",
            SceneKind::Dialogue => "dialogue",
            SceneKind::Cutscene => "cutscene",
            SceneKind::Defeat => "defeat",
            SceneKind::Victory => "victory",
            SceneKind::Overlay => "overlay",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SceneLabel {
    pub kind: SceneKind,
    pub confidence: Confidence,
    /// Why, in a few words.
    pub reason: String,
}

/// Something that moved: a tracked blob of change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedObject {
    /// Stable across frames while the tracker keeps it.
    pub id: u64,
    pub rect: Rect,
    /// Pixels per analysed frame.
    pub velocity: (f32, f32),
    pub age_frames: u32,
    /// Not seen this frame; position predicted from its motion.
    pub predicted: bool,
    pub confidence: Confidence,
    /// Its most common colour.
    pub color: [u8; 3],
}

/// Text read on screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextItem {
    pub text: String,
    pub rect: Rect,
    /// The UI region it sits in, if any.
    pub region: Option<u32>,
    pub confidence: Confidence,
    /// Which OCR engine read it (`windows`, `tesseract`, a plugin...).
    pub engine: String,
    /// Read on this frame (false: carried from an earlier read of an unchanged region).
    pub fresh: bool,
}

/// What a stable piece of interface looks like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiKind {
    /// A bar: how full, of what colour, which way it fills.
    Bar {
        fill: f32,
        color: [u8; 3],
        vertical: bool,
    },
    /// A roughly square panel near an edge whose inside keeps changing a little.
    Minimap,
    /// A panel of text lines (chat, log, objectives).
    TextPanel,
    /// A small square that recurs (skill, item, buff icons).
    Icon,
    /// A plain panel.
    Panel,
    /// Stable, but not yet understood.
    Unknown,
}

impl UiKind {
    pub fn word(&self) -> &'static str {
        match self {
            UiKind::Bar { .. } => "bar",
            UiKind::Minimap => "minimap",
            UiKind::TextPanel => "text panel",
            UiKind::Icon => "icon",
            UiKind::Panel => "panel",
            UiKind::Unknown => "element",
        }
    }
}

/// A piece of interface: something that stays put while the game moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiRegion {
    /// Stable across frames and, through the profile, across sessions.
    pub id: u32,
    pub rect: Rect,
    pub norm: NormRect,
    pub kind: UiKind,
    /// How much of the time it has stayed put while the rest of the screen changed.
    pub stability: f32,
    pub confidence: Confidence,
    /// A 64-bit perceptual hash of how it looks (for recognition and recurring elements).
    pub appearance: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterRole {
    /// Probably the player's own character (stays near where the camera follows).
    PlayerCandidate,
    /// Another moving character or creature.
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterHypothesis {
    pub object: u64,
    pub role: CharacterRole,
    pub confidence: Confidence,
    pub reason: String,
}

/// Something that happened between the previous analysed frame and this one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservedEvent {
    SceneChanged {
        from: SceneKind,
        to: SceneKind,
    },
    RegionAppeared {
        region: u32,
    },
    RegionDisappeared {
        region: u32,
    },
    TextChanged {
        region: Option<u32>,
        text: String,
    },
    /// The whole screen went much darker or brighter at once.
    Flash {
        brightness_delta: f32,
    },
    /// The screen turned red or grey and the colour drained out: often a death.
    ColorDrain {
        red: f32,
        saturation: f32,
    },
}

/// A reference to one element of an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementRef {
    Region(u32),
    Text(usize),
    Object(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// Text naming an element (the "HP" next to a bar).
    Labels,
    /// Inside it.
    Inside,
    /// Next to it.
    Near,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub from: ElementRef,
    pub rel: RelationKind,
    pub to: ElementRef,
}

/// Something perception could not do or is unsure of, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uncertainty {
    pub about: String,
    pub reason: String,
}

/// Whole-frame measurements, cheap and always present.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct FrameMetrics {
    /// Fraction of the frame that changed since the last analysed frame.
    pub change: f32,
    /// Fraction of the frame covered by moving blobs.
    pub motion: f32,
    /// Mean brightness, 0..1.
    pub brightness: f32,
    /// Mean saturation, 0..1.
    pub saturation: f32,
    /// How much redder than green/blue the frame is on average, 0..1.
    pub red_tint: f32,
    /// Fraction of strong edges (detail), 0..1.
    pub detail: f32,
}

/// A compact description of what the scene looks like, for "have I been here before?".
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SceneSignature {
    /// 64-bit difference hash of the frame without its stable interface.
    pub hash: u64,
    /// 12 hue bins and 4 brightness bins, normalised.
    pub histogram: [f32; 16],
}

impl SceneSignature {
    /// 0 (identical) to 1 (nothing alike).
    pub fn distance(&self, other: &SceneSignature) -> f32 {
        let bits = (self.hash ^ other.hash).count_ones() as f32 / 64.0;
        let hist: f32 = self
            .histogram
            .iter()
            .zip(other.histogram.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 2.0;
        (0.5 * bits + 0.5 * hist).clamp(0.0, 1.0)
    }
}

/// Everything one analysed frame showed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Observation {
    pub frame_index: u64,
    pub timestamp_ms: u64,
    pub frame_size: (u32, u32),
    pub scene: SceneLabel,
    pub objects: Vec<ObservedObject>,
    pub text: Vec<TextItem>,
    pub ui_regions: Vec<UiRegion>,
    pub characters: Vec<CharacterHypothesis>,
    pub events: Vec<ObservedEvent>,
    pub relationships: Vec<Relationship>,
    pub uncertainties: Vec<Uncertainty>,
    pub metrics: FrameMetrics,
    pub signature: SceneSignature,
    /// How long the analysis took, in milliseconds.
    pub analysis_ms: f32,
}

impl Observation {
    pub fn region(&self, id: u32) -> Option<&UiRegion> {
        self.ui_regions.iter().find(|r| r.id == id)
    }

    /// All text read on screen, joined, lower case (for keyword checks).
    pub fn all_text_lower(&self) -> String {
        self.text
            .iter()
            .map(|t| t.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" \n")
    }

    /// Text items labelling (or inside) a region.
    pub fn labels_of(&self, region: u32) -> Vec<&TextItem> {
        let mut out: Vec<&TextItem> = self
            .relationships
            .iter()
            .filter(|r| {
                r.to == ElementRef::Region(region)
                    && matches!(r.rel, RelationKind::Labels | RelationKind::Inside)
            })
            .filter_map(|r| match r.from {
                ElementRef::Text(i) => self.text.get(i),
                _ => None,
            })
            .collect();
        for t in &self.text {
            if t.region == Some(region) && !out.iter().any(|o| std::ptr::eq(*o, t)) {
                out.push(t);
            }
        }
        out
    }

    pub fn uncertain(&mut self, about: impl Into<String>, reason: impl Into<String>) {
        self.uncertainties.push(Uncertainty {
            about: about.into(),
            reason: reason.into(),
        });
    }
}
