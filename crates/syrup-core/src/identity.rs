//! Which game is this, how sure, and why.

use serde::{Deserialize, Serialize};

use crate::confidence::Confidence;
use crate::geometry::NormRect;

/// One piece of evidence for a game's identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// `window_title`, `executable`, `screen_text`, `hud_signature`,
    /// `steam_folder`, `confirmation`, `plugin`, `catalog`...
    pub signal: String,
    pub detail: String,
    pub weight: Confidence,
}

/// The answer to "what game is this?".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameIdentity {
    /// Stable key for the game's memory: `maplestory`, `elden-ring`, or
    /// `unknown-<hash>` for a game Syrup could not name yet.
    pub game_id: String,
    pub title: String,
    pub confidence: Confidence,
    pub version: Option<String>,
    pub platform: Option<String>,
    pub evidence: Vec<Evidence>,
    /// The player confirmed it.
    pub confirmed: bool,
}

impl GameIdentity {
    pub fn is_known(&self) -> bool {
        !self.game_id.starts_with("unknown")
    }

    /// Named confidently enough to say so out loud.
    pub fn is_confident(&self) -> bool {
        self.confirmed || (self.is_known() && self.confidence.at_least(0.75))
    }
}

/// A stable piece of interface as a fingerprint: where it is and how it looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HudMark {
    pub norm: NormRect,
    pub appearance: u64,
    pub kind: String,
}

/// Everything the recogniser can use.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct IdentityCues {
    pub window_title: Option<String>,
    pub executable: Option<String>,
    pub executable_path: Option<String>,
    /// Text read on screen lately (title screens and menus often name the game).
    pub screen_text: Vec<String>,
    /// The stable interface seen so far.
    pub hud: Vec<HudMark>,
}
