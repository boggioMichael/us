//! What each concept looks like, in any game: the words that name it, the
//! colours and places it tends to have, and how it behaves. These are
//! priors, not rules: evidence from the game itself (and the player) wins.

use syrup_core::state::ConceptUnit;

pub struct ConceptSpec {
    pub name: &'static str,
    /// Words that name it on screen (lower case, several languages where common).
    pub labels: &'static [&'static str],
    pub unit: ConceptUnit,
    /// Something that drains and refills (health, mana...), worth warning about when low.
    pub resource: bool,
}

pub const CONCEPTS: &[ConceptSpec] = &[
    ConceptSpec {
        name: "health",
        labels: &["hp", "health", "life", "vida", "vie", "leben", "vit", "hit points", "vitality", "hull", "integrity"],
        unit: ConceptUnit::Fraction,
        resource: true,
    },
    ConceptSpec {
        name: "mana",
        labels: &["mp", "mana", "magic", "spirit", "focus", "fp"],
        unit: ConceptUnit::Fraction,
        resource: true,
    },
    ConceptSpec {
        name: "stamina",
        labels: &["stamina", "sp", "st", "endurance", "fatigue", "breath"],
        unit: ConceptUnit::Fraction,
        resource: true,
    },
    ConceptSpec {
        name: "energy",
        labels: &["energy", "en", "power", "rage", "fury", "fuel", "battery", "charge", "boost", "nitro"],
        unit: ConceptUnit::Fraction,
        resource: true,
    },
    ConceptSpec {
        name: "shield",
        labels: &["shield", "shields", "armor", "armour", "barrier", "guard", "def"],
        unit: ConceptUnit::Fraction,
        resource: true,
    },
    ConceptSpec {
        name: "experience",
        labels: &["exp", "xp", "experience", "ex"],
        unit: ConceptUnit::Fraction,
        resource: false,
    },
    ConceptSpec { name: "boss_health", labels: &["boss"], unit: ConceptUnit::Fraction, resource: false },
    ConceptSpec {
        name: "ammo",
        labels: &["ammo", "bullets", "rounds", "mag", "magazine", "clip", "arrows", "shells"],
        unit: ConceptUnit::Count,
        resource: true,
    },
    ConceptSpec {
        name: "currency",
        labels: &[
            "gold", "coins", "coin", "money", "cash", "credits", "gems", "mesos", "meso", "zeny", "rupees", "$",
            "souls", "runes", "bells", "silver", "g",
        ],
        unit: ConceptUnit::Count,
        resource: false,
    },
    ConceptSpec {
        name: "score",
        labels: &["score", "points", "pts", "hiscore", "high score"],
        unit: ConceptUnit::Count,
        resource: false,
    },
    ConceptSpec { name: "level", labels: &["lv", "lvl", "level"], unit: ConceptUnit::Count, resource: false },
    ConceptSpec {
        name: "timer",
        labels: &["time", "timer", "time left", "time remaining", "remaining", "clock"],
        unit: ConceptUnit::Seconds,
        resource: false,
    },
    ConceptSpec {
        name: "round",
        labels: &["round", "wave", "stage", "turn", "lap", "floor"],
        unit: ConceptUnit::Count,
        resource: false,
    },
    ConceptSpec { name: "lives", labels: &["lives", "life x"], unit: ConceptUnit::Count, resource: true },
    ConceptSpec {
        name: "kills",
        labels: &["kills", "kill", "eliminations", "frags", "ko"],
        unit: ConceptUnit::Count,
        resource: false,
    },
    ConceptSpec {
        name: "objective",
        labels: &["quest", "mission", "objective", "goal", "task"],
        unit: ConceptUnit::Text,
        resource: false,
    },
];

pub fn spec(name: &str) -> Option<&'static ConceptSpec> {
    CONCEPTS.iter().find(|c| c.name == name)
}

/// The concept a label names, if any.
pub fn concept_for_label(label: &str) -> Option<&'static str> {
    let label = label.trim().to_lowercase();
    if let Some(c) = CONCEPTS.iter().find(|c| c.labels.contains(&label.as_str())) {
        return Some(c.name);
    }
    // OCR often glues a stray stroke to a short label ("hpl", "mp1").
    if label.len() >= 3 {
        let trimmed: String = label.chars().take(label.chars().count() - 1).collect();
        if trimmed.len() >= 2 && label.ends_with(['l', 'i', '1', '|', 'j']) {
            return CONCEPTS.iter().find(|c| c.labels.contains(&trimmed.as_str())).map(|c| c.name);
        }
    }
    None
}

/// Verbs and nouns of objectives ("Defeat slimes (4/10)", "Reach the tower").
pub const OBJECTIVE_WORDS: &[&str] = &[
    "quest",
    "mission",
    "objective",
    "defeat",
    "collect",
    "find",
    "reach",
    "talk",
    "escape",
    "survive",
    "protect",
    "destroy",
    "kill",
    "deliver",
    "gather",
    "clear",
    "hunt",
    "rescue",
    "go to",
    "return",
];

/// Priors for a bar of hue `hue` (degrees): (concept, weight).
pub fn color_priors(hue: f32, chroma: u8) -> Vec<(&'static str, f32)> {
    if chroma < 60 {
        return vec![("shield", 0.15), ("stamina", 0.1)];
    }
    match hue {
        h if !(20.0..340.0).contains(&h) => vec![("health", 0.4), ("boss_health", 0.1)],
        h if h < 45.0 => vec![("energy", 0.2), ("experience", 0.15), ("health", 0.1), ("stamina", 0.1)],
        h if h < 70.0 => vec![("experience", 0.3), ("stamina", 0.15), ("energy", 0.15), ("timer", 0.1)],
        h if h < 160.0 => vec![("health", 0.25), ("stamina", 0.2), ("experience", 0.1), ("boss_health", 0.1)],
        h if h < 200.0 => vec![("mana", 0.25), ("shield", 0.2)],
        h if h < 250.0 => vec![("mana", 0.35), ("shield", 0.15)],
        h if h < 300.0 => vec![("mana", 0.2), ("boss_health", 0.15), ("experience", 0.1)],
        _ => vec![("health", 0.3), ("mana", 0.1)],
    }
}
