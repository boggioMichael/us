//! Three very different games, one data folder, two sessions each.
//!
//! The first session learns each game from nothing; the second must start
//! from what the first learned. Measured against each game's truth. With an
//! OCR engine the bar is higher (text names things); without one Syrup still
//! has to learn from colours, shapes and behaviour.

use std::path::PathBuf;

use syrup_core::Hat;
use syrup_e2e::{has_ocr, ocr, play, runtime};
use syrup_testgames::GameKind;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/syrup-knowledge/fixtures")
}

#[test]
fn syrup_learns_three_different_games_and_remembers_them() {
    // As in the program: room on the stack (Windows' default thread stack is small).
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(three_games)
        .expect("a thread")
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
}

fn three_games() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let with_ocr = has_ocr();
    eprintln!("OCR: {}", ocr().name());
    let mut ids = Vec::new();

    // --- First sessions: nothing known. ---
    // A first-person dungeon: a health bar, counters, a minimap, motion everywhere.
    let mut rt = runtime(data, ocr(), None);
    let d = play(&mut rt, GameKind::Dungeon, 1, 70.0);
    eprintln!("{}", d.describe("dungeon"));
    assert_eq!(rt.identity().unwrap().title, "Dungeon 3D");
    ids.push(rt.identity().unwrap().game_id.clone());
    // Without text (or with an engine that does not read the lone "HP" beside
    // it, as Windows' own may not), the red bar is taken for health from its
    // colour and how it behaves, which takes longer to be sure of.
    let health = &d.score.concepts["health"];
    let reads_labels = with_ocr && ocr().name() != "windows";
    let (known, error) = if reads_labels { (0.7, 0.1) } else { (0.4, 0.12) };
    assert!(health.coverage() >= known, "dungeon health known {:.2}", health.coverage());
    assert!(health.mean_error().unwrap() <= error, "dungeon health error {:?}", health.mean_error());
    assert!(d.score.scene_accuracy() >= 0.8, "dungeon scene {:.2}", d.score.scene_accuracy());
    assert_eq!(d.report.summary.deaths as usize, d.happened.defeats, "dungeon deaths");
    drop(rt);

    // An MMO-style side-scroller: HP, MP and EXP bars, a boss with its own bar,
    // a death dialog, a level-up; looked up (from recorded pages) as it starts.
    let mut rt = runtime(data, ocr(), Some(&fixtures()));
    let s = play(&mut rt, GameKind::Scroller, 1, 75.0);
    eprintln!("{}", s.describe("scroller"));
    ids.push(rt.identity().unwrap().game_id.clone());
    for (concept, coverage) in [("health", 0.7), ("experience", 0.7), ("mana", if reads_labels { 0.6 } else { 0.2 })] {
        let c = &s.score.concepts[concept];
        assert!(c.coverage() >= coverage, "scroller {concept} known {:.2}", c.coverage());
        assert!(c.mean_error().unwrap_or(1.0) <= 0.08, "scroller {concept} error {:?}", c.mean_error());
    }
    assert_eq!(s.report.summary.victories, 0, "no false victories");
    // The death dialog is mostly words; without reading them it can be missed.
    assert!(s.report.summary.deaths as usize <= s.happened.defeats, "scroller: deaths that did not happen");
    if with_ocr {
        assert_eq!(s.report.summary.deaths as usize, s.happened.defeats, "scroller deaths");
        assert!(s.score.scene_accuracy() >= 0.8, "scroller scene {:.2}", s.score.scene_accuracy());
        assert_eq!(s.report.summary.level_ups as usize, s.happened.level_ups, "scroller level-ups");
    }
    // Research found the game: genres, a version, a hat for it.
    let p = rt.profile().unwrap();
    assert!(p.genres.iter().any(|g| g == "mmorpg"), "{:?}", p.genres);
    assert_ne!(p.visual_identity.hat, Hat::SyrupCap);
    assert!(rt.knowledge().unwrap().facts.len() >= 4);
    drop(rt);

    // A card game: menus, a still table, a turn timer, results.
    let mut rt = runtime(data, ocr(), None);
    let c = play(&mut rt, GameKind::Cards, 1, 60.0);
    eprintln!("{}", c.describe("cards"));
    assert_eq!(rt.identity().unwrap().title, "High Card Duel");
    ids.push(rt.identity().unwrap().game_id.clone());
    assert_eq!(c.report.summary.victories as usize, c.happened.wins, "cards: wins");
    if with_ocr {
        assert!(c.score.scene_accuracy() >= 0.5, "cards scene {:.2}", c.score.scene_accuracy());
        let learned = &c.report.summary.concepts;
        assert!(learned.iter().any(|x| x == "score" || x == "round"), "{learned:?}");
    }
    drop(rt);

    // Three games, three profiles.
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "{ids:?}");
    let store = syrup_memory_store(data);
    assert_eq!(store.len(), 3);

    // --- Second sessions: each game is recognised at once and picked up where it was. ---
    for (kind, title) in [
        (GameKind::Dungeon, "Dungeon 3D"),
        (GameKind::Scroller, "Sky Meadow Online"),
        (GameKind::Cards, "High Card Duel"),
    ] {
        let mut rt = runtime(data, ocr(), None);
        let again = play(&mut rt, kind, 9, 12.0);
        let id = rt.identity().unwrap();
        assert_eq!(id.title, title);
        assert!(id.is_confident(), "{title}: {id:?}");
        let p = rt.profile().unwrap();
        assert_eq!(p.stats.sessions, 2, "{title}");
        let said: Vec<String> = again.report.said.iter().map(|(_, t)| t.clone()).collect();
        assert!(said.iter().any(|t| t.contains("again")), "{title}: {said:?}");
        // What it knew from last time is known from the start.
        assert!(!rt.state().concepts.is_empty(), "{title}: nothing known after 12 s");
    }
}

fn syrup_memory_store(data: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(data.join("games"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}
