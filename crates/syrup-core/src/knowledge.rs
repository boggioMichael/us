//! Knowledge about a game: facts with provenance, and the graph they build.

use serde::{Deserialize, Serialize};

use crate::confidence::Confidence;

/// What kind of claim a fact is. Advice says which it rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    /// Stated by an authoritative source.
    Fact,
    /// About the current patch or version (balance numbers, patch notes).
    CurrentPatch,
    /// What players generally agree on.
    CommunityConsensus,
    /// Rumour, theory, "might".
    Speculation,
    /// Syrup's own conclusion from what it watched.
    Inference,
}

impl FactKind {
    pub fn word(self) -> &'static str {
        match self {
            FactKind::Fact => "fact",
            FactKind::CurrentPatch => "current patch",
            FactKind::CommunityConsensus => "community consensus",
            FactKind::Speculation => "speculation",
            FactKind::Inference => "my own inference",
        }
    }
}

/// Where knowledge comes from, roughly in order of authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Official,
    PatchNotes,
    GameDatabase,
    Wiki,
    Encyclopedia,
    Guide,
    Community,
    Forum,
    Video,
    Observation,
    Plugin,
    Player,
}

impl SourceKind {
    /// How much a claim from this kind of source is worth before anything else is known.
    pub fn authority(self) -> f32 {
        match self {
            SourceKind::Official | SourceKind::PatchNotes => 0.95,
            SourceKind::Player => 0.95,
            SourceKind::GameDatabase => 0.85,
            SourceKind::Wiki => 0.8,
            SourceKind::Encyclopedia => 0.75,
            SourceKind::Plugin => 0.75,
            SourceKind::Guide => 0.65,
            SourceKind::Observation => 0.6,
            SourceKind::Community => 0.55,
            SourceKind::Video => 0.5,
            SourceKind::Forum => 0.45,
        }
    }
}

/// A source Syrup may consult for a game.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeSource {
    pub kind: SourceKind,
    /// `wikipedia`, `fandom`, `steam`, `reddit`, `official`, `patch_notes`...
    pub provider: String,
    /// What to look up there: an article title, a wiki domain, an app id, a URL.
    pub locator: String,
}

/// How much of the story a fact gives away.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum SpoilerLevel {
    /// Gives nothing away: controls, mechanics, general tips.
    #[default]
    None,
    /// Hints at what is ahead.
    Mild,
    /// Reveals story, bosses or solutions.
    Major,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub title: String,
    pub url: String,
}

/// One claim, with where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub claim: String,
    /// The node it is mainly about, when known.
    pub subject: Option<String>,
    pub kind: FactKind,
    pub source: SourceRef,
    /// ISO 8601.
    pub retrieved_at: String,
    /// The game version it applies to, when stated.
    pub game_version: Option<String>,
    pub confidence: Confidence,
    pub spoiler: SpoilerLevel,
    /// Set when the game has moved on since (a newer version than the fact's).
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Game,
    Item,
    Character,
    Enemy,
    Boss,
    Quest,
    Skill,
    Map,
    Resource,
    Weapon,
    Mechanic,
    Strategy,
    Build,
    Faction,
    Location,
    Objective,
    Concept,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeNode {
    /// Lower-case, hyphenated name: `fire-sword`.
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    pub aliases: Vec<String>,
    pub summary: Option<String>,
    /// Fact ids that mention it.
    pub facts: Vec<String>,
    pub provenance: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    EffectiveAgainst,
    WeakAgainst,
    RequiredFor,
    SynergizesWith,
    LocatedIn,
    Drops,
    PartOf,
    Counters,
    Unlocks,
    RelatedTo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub from: String,
    pub rel: Relation,
    pub to: String,
    pub confidence: Confidence,
    /// The fact the edge was read from.
    pub fact: Option<String>,
    pub provenance: String,
}

/// Turn a name into a node id: `Fire Sword` → `fire-sword`.
pub fn node_id(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_are_stable() {
        assert_eq!(node_id("Fire Sword"), "fire-sword");
        assert_eq!(node_id("  Zakum's Arm (Chaos) "), "zakum-s-arm-chaos");
        assert_eq!(node_id("HP"), "hp");
    }
}
