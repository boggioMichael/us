//! What Syrup has learned about one game, kept between sessions.
//!
//! A profile starts almost empty (an identity, maybe genres from the catalog)
//! and fills in as Syrup watches: which stable elements the interface has and
//! what each one is, which icons recur, which words appear, what the player
//! corrected. Plugins may seed a profile; none is required.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::confidence::Confidence;
use crate::geometry::NormRect;
use crate::identity::HudMark;
use crate::knowledge::KnowledgeSource;

/// Syrup's hat: the one visual thing that changes from game to game.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum Hat {
    /// The original Maplesyrup cap: a white cap with the orange S under a stack of syrupy pancakes.
    #[default]
    SyrupCap,
    WizardHat,
    KnightHelmet,
    RangerHood,
    RacingHelmet,
    TacticalHelmet,
    AstronautHelmet,
    PirateHat,
    StrawHat,
    DetectiveCap,
    DealerVisor,
    SportsCap,
    LanternHat,
}

impl Hat {
    pub const ALL: [Hat; 13] = [
        Hat::SyrupCap,
        Hat::WizardHat,
        Hat::KnightHelmet,
        Hat::RangerHood,
        Hat::RacingHelmet,
        Hat::TacticalHelmet,
        Hat::AstronautHelmet,
        Hat::PirateHat,
        Hat::StrawHat,
        Hat::DetectiveCap,
        Hat::DealerVisor,
        Hat::SportsCap,
        Hat::LanternHat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Hat::SyrupCap => "syrup_cap",
            Hat::WizardHat => "wizard_hat",
            Hat::KnightHelmet => "knight_helmet",
            Hat::RangerHood => "ranger_hood",
            Hat::RacingHelmet => "racing_helmet",
            Hat::TacticalHelmet => "tactical_helmet",
            Hat::AstronautHelmet => "astronaut_helmet",
            Hat::PirateHat => "pirate_hat",
            Hat::StrawHat => "straw_hat",
            Hat::DetectiveCap => "detective_cap",
            Hat::DealerVisor => "dealer_visor",
            Hat::SportsCap => "sports_cap",
            Hat::LanternHat => "lantern_hat",
        }
    }

    pub fn from_name(name: &str) -> Option<Hat> {
        Hat::ALL.into_iter().find(|h| h.name() == name)
    }

    /// The hat for a game of these genres (tags such as `rpg`, `racing`,
    /// `shooter`), when no profile or plugin chose one.
    pub fn for_genres<S: AsRef<str>>(genres: &[S]) -> Hat {
        let has = |tags: &[&str]| genres.iter().any(|g| tags.contains(&g.as_ref()));
        if has(&["racing", "driving"]) {
            Hat::RacingHelmet
        } else if has(&["space", "sci_fi_space", "space_sim"]) {
            Hat::AstronautHelmet
        } else if has(&["pirate", "naval"]) {
            Hat::PirateHat
        } else if has(&["horror", "survival_horror"]) {
            Hat::LanternHat
        } else if has(&["farming", "life_sim", "cozy"]) {
            Hat::StrawHat
        } else if has(&["mystery", "detective", "investigation"]) {
            Hat::DetectiveCap
        } else if has(&["card", "cards", "deckbuilder", "poker", "board"]) {
            Hat::DealerVisor
        } else if has(&["sports", "football", "soccer", "basketball"]) {
            Hat::SportsCap
        } else if has(&["shooter", "fps", "military", "tactical", "battle_royale"]) {
            Hat::TacticalHelmet
        } else if has(&["stealth", "archery", "survival", "open_world"]) {
            Hat::RangerHood
        } else if has(&[
            "soulslike",
            "action_rpg",
            "hack_and_slash",
            "dungeon",
            "medieval",
            "strategy",
        ]) {
            Hat::KnightHelmet
        } else if has(&["rpg", "fantasy", "jrpg", "mmo", "mmorpg", "magic"]) {
            Hat::WizardHat
        } else {
            Hat::SyrupCap
        }
    }
}

/// How Syrup looks in this game.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VisualIdentity {
    pub hat: Hat,
    /// An accent colour for Syrup's card in this game, if the game has a strong one.
    pub accent: Option<[u8; 3]>,
}

/// A piece of the game's interface Syrup knows about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownUiElement {
    /// Stable id within the profile.
    pub id: u32,
    pub norm: NormRect,
    /// `bar`, `minimap`, `text panel`, `icon`, `panel`, `element`.
    pub kind: String,
    /// What it is believed to be: `health`, `mana`, `score`...
    pub concept: Option<String>,
    pub appearance: u64,
    /// How many analysed frames it was seen in.
    pub seen: u64,
    pub confidence: Confidence,
    /// The player named or corrected it: outranks any inference.
    pub corrected: bool,
    /// Where it came from: `observed`, `plugin:maplestory`, `player`.
    pub origin: String,
}

/// A recurring small visual pattern (an icon, a symbol).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualWord {
    pub appearance: u64,
    pub seen: u64,
    pub first_seen_ms: u64,
    /// What it is, once known (from the player, a plugin, or text next to it).
    pub label: Option<String>,
    /// Typical size, in frame fractions.
    pub size: (f32, f32),
}

/// Something known about the game, in a profile's lists of mechanics,
/// entities, resources, objectives, actions, states, progression systems.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileItem {
    pub name: String,
    pub description: String,
    pub confidence: Confidence,
    /// `observed`, `research`, `plugin:<id>`, `player`.
    pub provenance: String,
}

/// A word seen on screen, and how often.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TermStats {
    pub count: u64,
    pub last_seen_ms: u64,
}

/// A correction the player made; replayed on every load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    /// `identity`, `ui_element:<id>`, `concept:<name>`...
    pub target: String,
    pub value: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProfileStats {
    pub sessions: u64,
    pub observed_ms: u64,
    pub analysed_frames: u64,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameProfile {
    pub schema: u32,
    pub game_id: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub executables: Vec<String>,
    pub window_titles: Vec<String>,
    pub current_version: Option<String>,
    pub genres: Vec<String>,
    pub known_ui_elements: Vec<KnownUiElement>,
    pub visual_vocabulary: Vec<VisualWord>,
    pub mechanics: Vec<ProfileItem>,
    pub entities: Vec<ProfileItem>,
    pub resources: Vec<ProfileItem>,
    pub objectives: Vec<ProfileItem>,
    pub player_actions: Vec<ProfileItem>,
    pub common_states: Vec<ProfileItem>,
    pub progression_systems: Vec<ProfileItem>,
    pub knowledge_sources: Vec<KnowledgeSource>,
    pub terminology: BTreeMap<String, TermStats>,
    pub strategy_knowledge: Vec<ProfileItem>,
    pub visual_identity: VisualIdentity,
    /// The stable interface as a fingerprint, for recognising the game by sight.
    pub hud_signature: Vec<HudMark>,
    pub corrections: Vec<Correction>,
    pub stats: ProfileStats,
}

impl GameProfile {
    pub const SCHEMA: u32 = 1;

    pub fn new(game_id: impl Into<String>, title: impl Into<String>) -> Self {
        GameProfile {
            schema: Self::SCHEMA,
            game_id: game_id.into(),
            title: title.into(),
            aliases: Vec::new(),
            executables: Vec::new(),
            window_titles: Vec::new(),
            current_version: None,
            genres: Vec::new(),
            known_ui_elements: Vec::new(),
            visual_vocabulary: Vec::new(),
            mechanics: Vec::new(),
            entities: Vec::new(),
            resources: Vec::new(),
            objectives: Vec::new(),
            player_actions: Vec::new(),
            common_states: Vec::new(),
            progression_systems: Vec::new(),
            knowledge_sources: Vec::new(),
            terminology: BTreeMap::new(),
            strategy_knowledge: Vec::new(),
            visual_identity: VisualIdentity::default(),
            hud_signature: Vec::new(),
            corrections: Vec::new(),
            stats: ProfileStats::default(),
        }
    }

    pub fn element_for_concept(&self, concept: &str) -> Option<&KnownUiElement> {
        self.known_ui_elements
            .iter()
            .filter(|e| e.concept.as_deref() == Some(concept))
            .max_by(|a, b| a.confidence.value().total_cmp(&b.confidence.value()))
    }

    /// The most frequent words read on screen, most frequent first.
    pub fn top_terms(&self, n: usize) -> Vec<(&str, u64)> {
        let mut terms: Vec<(&str, u64)> = self
            .terminology
            .iter()
            .map(|(k, v)| (k.as_str(), v.count))
            .collect();
        terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        terms.truncate(n);
        terms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hats_follow_genres_and_round_trip_by_name() {
        assert_eq!(Hat::for_genres(&["racing"]), Hat::RacingHelmet);
        assert_eq!(Hat::for_genres(&["rpg", "fantasy"]), Hat::WizardHat);
        assert_eq!(Hat::for_genres(&["shooter", "fps"]), Hat::TacticalHelmet);
        assert_eq!(Hat::for_genres(&["card", "roguelike"]), Hat::DealerVisor);
        assert_eq!(Hat::for_genres::<&str>(&[]), Hat::SyrupCap);
        for h in Hat::ALL {
            assert_eq!(Hat::from_name(h.name()), Some(h));
        }
        let json = serde_json::to_string(&Hat::PirateHat).unwrap();
        assert_eq!(json, "\"pirate_hat\"");
    }
}
