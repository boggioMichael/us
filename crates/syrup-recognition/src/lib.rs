//! Which game is this?
//!
//! [`Recognizer`] weighs independent clues with a noisy-OR: the executable
//! and window title against the catalog and against every profile Syrup has
//! learned, the Steam library folder the game runs from, the game's name read
//! on screen (title screens and menus say it), the HUD's layout against each
//! learned game's HUD signature, and the player's confirmation. It returns
//! the best candidate with every piece of evidence. Below the confidence bar
//! the answer is "not sure yet". A game nobody has catalogued still gets an
//! identity (`unknown-…`, from its executable or window), so Syrup can learn
//! it and the player can name it later.

pub mod catalog;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use syrup_core::util::{fnv64, hamming, normalize_words};
use syrup_core::{Confidence, Evidence, GameIdentity, GameProfile, HudMark, IdentityCues};

pub use catalog::{CATALOG, CatalogEntry, by_id};

/// Enough to name the game out loud.
pub const CONFIDENT: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct Recognition {
    /// The answer: a known game, or this window as a new, unnamed game.
    pub best: GameIdentity,
    /// Every candidate considered, best first.
    pub candidates: Vec<GameIdentity>,
}

impl Recognition {
    pub fn is_confident(&self) -> bool {
        self.best.is_confident()
    }
}

struct Candidate {
    id: String,
    title: String,
    names: Vec<String>,
    executables: Vec<String>,
    learned_titles: Vec<String>,
    hud: Vec<HudMark>,
}

fn exe_key(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let s = s.rsplit(['\\', '/']).next().unwrap_or(&s).to_string();
    s.strip_suffix(".exe").unwrap_or(&s).to_string()
}

/// The folder name under `steamapps/common/` in an executable's path.
pub fn steam_folder(path: &str) -> Option<String> {
    let lower = path.replace('\\', "/");
    let i = lower.to_lowercase().find("steamapps/common/")?;
    lower[i + "steamapps/common/".len()..].split('/').next().map(|s| s.to_string())
}

/// Where the game comes from, from its executable's path.
pub fn platform(path: &str) -> Option<String> {
    let p = path.to_lowercase().replace('\\', "/");
    let found = if p.contains("steamapps/") {
        "Steam"
    } else if p.contains("epic games") {
        "Epic Games"
    } else if p.contains("riot games") {
        "Riot"
    } else if p.contains("battle.net") || p.contains("blizzard") {
        "Battle.net"
    } else if p.contains("xboxgames") || p.contains("windowsapps") {
        "Xbox"
    } else if p.contains("gog galaxy") || p.contains("gog games") {
        "GOG"
    } else if p.contains("nexon") {
        "Nexon"
    } else {
        return None;
    };
    Some(found.to_string())
}

/// A version number read on screen ("v1.4.2", "Version 2.0.1", "Patch 14.3").
pub fn version_in(text: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:v|ver\.?|version|patch|build)\s*(\d+(?:\.\d+){1,3})\b").expect("valid pattern")
    });
    re.captures(text).map(|c| c[1].to_string())
}

/// A phrase occurs in normalised text as whole words.
fn contains_phrase(hay: &str, phrase: &str) -> bool {
    !phrase.is_empty() && format!(" {hay} ").contains(&format!(" {phrase} "))
}

/// How much of a learned HUD the current screen shows, 0..1.
pub fn hud_similarity(known: &[HudMark], now: &[HudMark]) -> f32 {
    if known.is_empty() || now.is_empty() {
        return 0.0;
    }
    let matched = known
        .iter()
        .filter(|k| {
            now.iter().any(|n| {
                n.kind == k.kind
                    && n.norm.iou(&k.norm) > 0.45
                    && (k.kind == "bar" && n.appearance == k.appearance
                        || k.kind != "bar" && hamming(n.appearance, k.appearance) <= 14)
            })
        })
        .count();
    matched as f32 / known.len() as f32
}

#[derive(Default)]
pub struct Recognizer {
    /// Evidence from on-screen text, which comes and goes, accumulated.
    text_hits: BTreeMap<String, f32>,
    confirmed: Option<(String, String)>,
}

impl Recognizer {
    pub fn new() -> Self {
        Recognizer::default()
    }

    /// The player said which game this is.
    pub fn confirm(&mut self, game_id: &str, title: &str) {
        self.confirmed = Some((game_id.to_string(), title.to_string()));
    }

    /// Forget accumulated screen-text evidence (a different window).
    pub fn reset(&mut self) {
        self.text_hits.clear();
        self.confirmed = None;
    }

    pub fn recognize(&mut self, cues: &IdentityCues, profiles: &[GameProfile]) -> Recognition {
        let mut candidates: Vec<Candidate> = CATALOG
            .iter()
            .map(|e| Candidate {
                id: e.id.to_string(),
                title: e.title.to_string(),
                names: std::iter::once(e.title).chain(e.aliases.iter().copied()).map(normalize_words).collect(),
                executables: e.executables.iter().map(|x| exe_key(x)).collect(),
                learned_titles: Vec::new(),
                hud: Vec::new(),
            })
            .collect();
        for p in profiles {
            let learned = |c: &mut Candidate| {
                c.executables.extend(p.executables.iter().map(|x| exe_key(x)));
                c.learned_titles.extend(p.window_titles.iter().map(|t| normalize_words(t)));
                c.names.extend(p.aliases.iter().map(|a| normalize_words(a)));
                c.hud = p.hud_signature.clone();
                if p.game_id.starts_with("unknown") || c.title.is_empty() {
                    c.title = p.title.clone();
                }
            };
            match candidates.iter_mut().find(|c| c.id == p.game_id) {
                Some(c) => learned(c),
                None => {
                    let mut c = Candidate {
                        id: p.game_id.clone(),
                        title: p.title.clone(),
                        names: if p.game_id.starts_with("unknown") {
                            Vec::new()
                        } else {
                            vec![normalize_words(&p.title)]
                        },
                        executables: Vec::new(),
                        learned_titles: Vec::new(),
                        hud: Vec::new(),
                    };
                    learned(&mut c);
                    candidates.push(c);
                }
            }
        }
        // A game the player named that nothing here knows yet (a phone game,
        // say) is a candidate of its own, named as the player named it.
        if let Some((id, title)) = &self.confirmed
            && !candidates.iter().any(|c| c.id == *id)
        {
            candidates.push(Candidate {
                id: id.clone(),
                title: title.clone(),
                names: vec![normalize_words(title)],
                executables: Vec::new(),
                learned_titles: Vec::new(),
                hud: Vec::new(),
            });
        }
        let exe = cues.executable.as_deref().map(exe_key);
        let title = cues.window_title.as_deref().map(normalize_words).unwrap_or_default();
        let steam = cues.executable_path.as_deref().and_then(steam_folder).map(|s| normalize_words(&s));
        let screen = normalize_words(&cues.screen_text.join(" \n "));
        let mut scored: Vec<GameIdentity> = Vec::new();
        for c in &candidates {
            let mut ev: Vec<Evidence> = Vec::new();
            let mut push = |signal: &str, detail: String, w: f32| {
                ev.push(Evidence { signal: signal.into(), detail, weight: Confidence::new(w) })
            };
            if let Some(exe) = &exe
                && c.executables.iter().any(|x| x == exe)
            {
                push("executable", format!("{exe}.exe"), 0.85);
            }
            if !title.is_empty() {
                if c.learned_titles.contains(&title) {
                    push("window_title", format!("\"{title}\" (seen before)"), 0.85);
                } else if let Some(n) = c.names.iter().find(|n| **n == title) {
                    push("window_title", format!("\"{n}\""), 0.8);
                } else if let Some(n) = c.names.iter().filter(|n| n.len() >= 4).find(|n| contains_phrase(&title, n)) {
                    push("window_title", format!("contains \"{n}\""), 0.65);
                }
            }
            if let Some(folder) = &steam
                && c.names.iter().any(|n| n == folder || contains_phrase(folder, n) && n.len() >= 5)
            {
                push("steam_folder", format!("steamapps/common/{folder}"), 0.6);
            }
            if let Some(n) =
                c.names.iter().filter(|n| n.len() >= 5 || n.contains(' ')).find(|n| contains_phrase(&screen, n))
            {
                let hits = self.text_hits.entry(c.id.clone()).or_insert(0.0);
                *hits = (*hits + 0.35).min(1.0);
                let _ = n;
            }
            if let Some(h) = self.text_hits.get(&c.id).copied().filter(|h| *h > 0.0) {
                push("screen_text", format!("its name was on screen ({:.0}%)", h * 100.0), (0.35 + 0.4 * h).min(0.75));
            }
            if c.hud.len() >= 3 {
                let sim = hud_similarity(&c.hud, &cues.hud);
                if sim >= 0.4 {
                    push("hud_signature", format!("{:.0}% of its interface is where it was", sim * 100.0), 0.7 * sim);
                }
            }
            if let Some((id, _)) = &self.confirmed
                && *id == c.id
            {
                push("confirmation", "the player said so".into(), 1.0);
            }
            if ev.is_empty() {
                continue;
            }
            let conf = Confidence::combine_all(ev.iter().map(|e| e.weight));
            scored.push(GameIdentity {
                game_id: c.id.clone(),
                title: c.title.clone(),
                confidence: conf,
                version: cues.screen_text.iter().find_map(|t| version_in(t)),
                platform: cues.executable_path.as_deref().and_then(platform),
                evidence: ev,
                confirmed: self.confirmed.as_ref().is_some_and(|(id, _)| *id == c.id),
            });
        }
        scored.sort_by(|a, b| b.confidence.value().total_cmp(&a.confidence.value()).then(a.game_id.cmp(&b.game_id)));
        // A close second means neither is sure.
        if scored.len() >= 2 {
            let margin = scored[0].confidence.value() - scored[1].confidence.value();
            if margin < 0.15 && !scored[0].confirmed {
                let c = scored[0].confidence.value() * (0.6 + margin * 2.0);
                scored[0].confidence = Confidence::new(c);
            }
        }
        let best = match scored.first() {
            Some(b) if b.confidence.at_least(0.5) || b.confirmed => b.clone(),
            _ => self.unknown(cues, profiles),
        };
        Recognition { best, candidates: scored }
    }

    /// This window as a game of its own, not named yet.
    fn unknown(&self, cues: &IdentityCues, profiles: &[GameProfile]) -> GameIdentity {
        let key = cues
            .executable
            .as_deref()
            .map(exe_key)
            .or_else(|| cues.window_title.as_deref().map(normalize_words))
            .unwrap_or_else(|| "screen".into());
        let id = format!("unknown-{:08x}", fnv64(key.as_bytes()) as u32);
        let known = profiles.iter().find(|p| p.game_id == id);
        let title = known.map(|p| p.title.clone()).unwrap_or_else(|| {
            cues.window_title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| cues.executable.clone().unwrap_or_else(|| "this game".into()))
        });
        let mut evidence = Vec::new();
        if let Some(exe) = &cues.executable {
            evidence.push(Evidence {
                signal: "executable".into(),
                detail: format!("{exe} (not in the catalog)"),
                weight: Confidence::new(0.5),
            });
        }
        if let Some(t) = &cues.window_title {
            evidence.push(Evidence {
                signal: "window_title".into(),
                detail: format!("\"{t}\""),
                weight: Confidence::new(0.3),
            });
        }
        GameIdentity {
            game_id: id,
            title,
            confidence: Confidence::new(if known.is_some() { 0.6 } else { 0.4 }),
            version: cues.screen_text.iter().find_map(|t| version_in(t)),
            platform: cues.executable_path.as_deref().and_then(platform),
            evidence,
            confirmed: false,
        }
    }
}

/// A new profile for a game, seeded from the catalog when it is in it.
pub fn new_profile(identity: &GameIdentity, cues: &IdentityCues) -> GameProfile {
    let mut p = GameProfile::new(identity.game_id.clone(), identity.title.clone());
    if let Some(e) = by_id(&identity.game_id) {
        p.aliases = e.aliases.iter().map(|s| s.to_string()).collect();
        p.executables = e.executables.iter().map(|s| s.to_string()).collect();
        p.genres = e.genres.iter().map(|s| s.to_string()).collect();
        p.visual_identity.hat = e.hat.unwrap_or_else(|| syrup_core::Hat::for_genres(e.genres));
        use syrup_core::knowledge::{KnowledgeSource, SourceKind};
        if let Some(api) = e.wiki_api {
            p.knowledge_sources.push(KnowledgeSource {
                kind: SourceKind::Wiki,
                provider: "mediawiki".into(),
                locator: api.into(),
            });
        }
        if let Some(w) = e.wikipedia {
            p.knowledge_sources.push(KnowledgeSource {
                kind: SourceKind::Encyclopedia,
                provider: "wikipedia".into(),
                locator: w.into(),
            });
        }
        if let Some(app) = e.steam_app {
            p.knowledge_sources.push(KnowledgeSource {
                kind: SourceKind::GameDatabase,
                provider: "steam".into(),
                locator: app.to_string(),
            });
        }
    }
    if let Some(exe) = &cues.executable
        && !p.executables.iter().any(|x| exe_key(x) == exe_key(exe))
    {
        p.executables.push(exe.clone());
    }
    if let Some(t) = &cues.window_title
        && !t.trim().is_empty()
    {
        p.window_titles.push(t.clone());
    }
    p.current_version = identity.version.clone();
    p
}

#[cfg(test)]
mod tests {
    use syrup_core::NormRect;

    use super::*;

    fn cues(title: &str, exe: &str, path: &str) -> IdentityCues {
        IdentityCues {
            window_title: Some(title.into()),
            executable: Some(exe.into()),
            executable_path: Some(path.into()),
            screen_text: Vec::new(),
            hud: Vec::new(),
        }
    }

    #[test]
    fn a_catalogued_game_is_named_by_its_window() {
        let mut r = Recognizer::new();
        let got = r.recognize(
            &cues("MapleStory", "MapleStory.exe", r"C:\Nexon\Library\maplestory\appdata\MapleStory.exe"),
            &[],
        );
        assert_eq!(got.best.game_id, "maplestory");
        assert!(got.is_confident(), "{got:?}");
        assert_eq!(got.best.platform.as_deref(), Some("Nexon"));
        let got = r.recognize(
            &cues("ELDEN RING™", "eldenring.exe", r"D:\SteamLibrary\steamapps\common\ELDEN RING\Game\eldenring.exe"),
            &[],
        );
        assert_eq!(got.best.game_id, "elden-ring");
        assert!(got.best.evidence.iter().any(|e| e.signal == "steam_folder"));
        assert_eq!(got.best.platform.as_deref(), Some("Steam"));
    }

    #[test]
    fn an_unknown_game_is_not_guessed_but_gets_an_identity() {
        let mut r = Recognizer::new();
        let got = r.recognize(&cues("Dungeon 3D", "dungeon3d.exe", r"C:\Games\Dungeon3D\dungeon3d.exe"), &[]);
        assert!(!got.best.is_known());
        assert!(!got.is_confident());
        assert!(got.best.game_id.starts_with("unknown-"));
        assert_eq!(got.best.title, "Dungeon 3D");
        // The same window next time gets the same identity.
        let again = Recognizer::new().recognize(&cues("Dungeon 3D", "dungeon3d.exe", ""), &[]);
        assert_eq!(again.best.game_id, got.best.game_id);
    }

    #[test]
    fn a_learned_profile_is_recognised_by_its_hud_and_executable() {
        let mut p = GameProfile::new("unknown-1234abcd", "Sky Meadow Online");
        p.executables.push("skymeadow.exe".into());
        let mark = |x: f32, y: f32, kind: &str, a: u64| HudMark {
            norm: NormRect { x, y, w: 0.2, h: 0.03 },
            appearance: a,
            kind: kind.into(),
        };
        p.hud_signature = vec![mark(0.2, 0.9, "bar", 1), mark(0.45, 0.9, "bar", 2), mark(0.0, 0.0, "minimap", 0xFFFF)];
        let mut c = cues("", "", "");
        c.window_title = None;
        c.executable = None;
        c.hud = p.hud_signature.clone();
        let got = Recognizer::new().recognize(&c, std::slice::from_ref(&p));
        assert_eq!(got.candidates[0].game_id, "unknown-1234abcd");
        assert!(got.candidates[0].evidence.iter().any(|e| e.signal == "hud_signature"));
        let c = cues("SkyMeadow", "SkyMeadow.EXE", "");
        let got = Recognizer::new().recognize(&c, &[p]);
        assert_eq!(got.best.game_id, "unknown-1234abcd");
        assert_eq!(got.best.title, "Sky Meadow Online");
    }

    #[test]
    fn the_name_on_screen_counts_and_versions_are_read() {
        let mut r = Recognizer::new();
        let mut c = cues("Game", "launcher.exe", "");
        c.screen_text = vec!["HOLLOW KNIGHT".into(), "v1.5.78".into()];
        let a = r.recognize(&c, &[]);
        let b = r.recognize(&c, &[]);
        let hk = |x: &Recognition| {
            x.candidates.iter().find(|c| c.game_id == "hollow-knight").map(|c| c.confidence.value()).unwrap_or(0.0)
        };
        assert!(hk(&b) >= hk(&a) && hk(&a) > 0.3);
        assert_eq!(b.best.version.as_deref(), Some("1.5.78"));
        r.confirm("hollow-knight", "Hollow Knight");
        assert!(r.recognize(&c, &[]).best.confirmed);
    }

    #[test]
    fn a_game_the_player_names_is_that_game_even_if_nothing_knows_it() {
        // A phone's screen: no window title, no executable.
        let blank = IdentityCues {
            window_title: None,
            executable: None,
            executable_path: None,
            screen_text: Vec::new(),
            hud: Vec::new(),
        };
        let mut r = Recognizer::new();
        assert_eq!(r.recognize(&blank, &[]).best.title, "this game");
        r.confirm("pocket-dungeon", "Pocket Dungeon");
        let got = r.recognize(&blank, &[]);
        assert_eq!((got.best.game_id.as_str(), got.best.title.as_str()), ("pocket-dungeon", "Pocket Dungeon"));
        assert!(got.best.confirmed && got.is_confident());
    }
}
