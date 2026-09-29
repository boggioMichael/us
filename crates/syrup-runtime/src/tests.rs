//! The whole pipeline on the synthetic games, with a data folder of its own.
//!
//! These run without OCR (fast, and what a player without Tesseract or
//! Windows OCR gets); `tests/` runs the same games with OCR end to end.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syrup_core::{Event, FeedbackKind, Hat};
use syrup_knowledge::Fixtures;
use syrup_perception::NoOcr;
use syrup_testgames::{GameKind, Session};

use super::*;
use crate::devtools::Devtools;
use crate::live::{LiveOptions, run};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../syrup-knowledge/fixtures")
}

fn runtime(dir: &Path) -> Runtime {
    Runtime::new(RuntimeConfig::new(dir, Arc::new(NoOcr))).unwrap()
}

fn play(rt: &mut Runtime, kind: GameKind, seed: u64, seconds: f32) -> live::LiveReport {
    let mut game = Session::of(kind, seed, 4.0, Some(seconds));
    run(rt, &mut game, &LiveOptions::default()).unwrap()
}

fn events(rt: &Runtime) -> Vec<Event> {
    rt.bus.since(0).into_iter().map(|r| r.event).collect()
}

#[test]
fn every_game_goes_through_the_whole_pipeline() {
    for (kind, title, seconds) in [
        (GameKind::Dungeon, "Dungeon 3D", 45.0),
        (GameKind::Scroller, "Sky Meadow Online", 45.0),
        (GameKind::Cards, "High Card Duel", 35.0),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = runtime(dir.path());
        let report = play(&mut rt, kind, 3, seconds);
        assert!(report.analysed >= 20, "{kind:?}: {} analysed", report.analysed);
        let id = rt.identity().unwrap_or_else(|| panic!("{kind:?}: no game")).clone();
        assert_eq!(id.title, title);
        // It learned the interface and kept it.
        let profile = rt.memory.load_profile(&id.game_id).unwrap_or_else(|| panic!("{kind:?}: no profile saved"));
        assert!(!profile.known_ui_elements.is_empty(), "{kind:?}: no elements");
        assert!(!profile.hud_signature.is_empty(), "{kind:?}: no HUD signature");
        assert_eq!(profile.stats.sessions, 1);
        let evs = events(&rt);
        for name in ["ui_element_discovered", "profile_updated", "advice_shown"] {
            assert!(evs.iter().any(|e| e.name() == name), "{kind:?}: no {name}");
        }
        assert!(evs.iter().any(|e| matches!(e, Event::GameUncertain { .. } | Event::GameIdentified { .. })));
        // It said hello, and the session's timeline and summary were written.
        assert!(!report.said.is_empty(), "{kind:?}: Syrup said nothing");
        let session = rt.memory.dir.session(&rt.session_id);
        let timeline = syrup_memory::read_timeline(&session.join("timeline.jsonl"));
        assert!(timeline.len() > 5 && !timeline.iter().any(|r| matches!(r.event, Event::FrameCaptured { .. })));
        assert!(session.join("summary.json").exists());
        assert!(report.summary.analysed_frames >= report.analysed);
        // The devtools snapshot mirrors it.
        let shared = rt.shared();
        let snap = shared.lock().unwrap().snapshot.clone();
        assert_eq!(snap.game.map(|g| g.title), Some(title.to_string()));
        assert!(snap.regions > 0 && snap.timings.contains_key("perception"));
        assert_eq!(rt.view.mode, OverlayMode::PostGame);
        assert!(!rt.view.summary.is_empty());
    }
}

#[test]
fn deaths_are_seen_remembered_and_talked_about() {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = runtime(dir.path());
    let mut game = Session::of(GameKind::Dungeon, 11, 4.0, Some(150.0));
    let truth = game.truth();
    let report = run(&mut rt, &mut game, &LiveOptions::default()).unwrap();
    let died =
        truth.lock().unwrap().events.iter().filter(|e| matches!(e.kind, syrup_testgames::TruthEventKind::Died)).count();
    assert!(died >= 1, "the test game should kill the player at least once");
    let seen = events(&rt).iter().filter(|e| matches!(e, Event::PlayerDied { .. })).count();
    assert!(seen >= 1, "{died} deaths, none seen");
    assert_eq!(report.summary.deaths as usize, seen);
    let id = rt.identity().unwrap().game_id.clone();
    assert!(rt.memory.episodes(&id).iter().any(|e| e.kind == "death"));
    assert!(rt.player().games.get(&id).is_some_and(|g| g.deaths >= 1));
}

#[test]
fn a_second_session_starts_from_what_the_first_learned() {
    let dir = tempfile::tempdir().unwrap();
    let first_id = {
        let mut rt = runtime(dir.path());
        play(&mut rt, GameKind::Scroller, 5, 40.0);
        rt.identity().unwrap().game_id.clone()
    };
    let mut rt = runtime(dir.path());
    let mut game = Session::of(GameKind::Scroller, 6, 4.0, Some(6.0));
    run(&mut rt, &mut game, &LiveOptions::default()).unwrap();
    let id = rt.identity().unwrap();
    assert_eq!(id.game_id, first_id, "the same game");
    // The executable it learned last time names the game at once, and Syrup says so.
    assert!(id.is_confident(), "{id:?}");
    assert!(
        rt.advice_log().any(|r| r.shown && r.advice.text.contains("again")),
        "{:?}",
        rt.advice_log().map(|r| &r.advice.text).collect::<Vec<_>>()
    );
    assert!(id.evidence.iter().any(|e| e.detail.contains("skymeadow")), "{:?}", id.evidence);
    let p = rt.profile().unwrap();
    assert_eq!(p.stats.sessions, 2);
    // Two different session folders.
    assert_eq!(std::fs::read_dir(rt.memory.dir.sessions()).unwrap().count(), 2);
}

#[test]
fn research_runs_beside_the_loop_and_dresses_syrup_for_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = RuntimeConfig::new(dir.path(), Arc::new(NoOcr));
    cfg.research = true;
    cfg.fetcher = Some(Arc::new(Fixtures::new(fixtures())));
    let mut rt = Runtime::new(cfg).unwrap();
    play(&mut rt, GameKind::Scroller, 2, 20.0);
    let g = rt.knowledge().expect("a knowledge graph");
    assert!(g.facts.len() >= 4, "{:#?}", g.facts);
    assert!(g.facts.iter().all(|f| f.source.url.starts_with("https://")));
    let evs = events(&rt);
    assert!(evs.iter().any(|e| matches!(e, Event::ResearchRequested { .. })));
    assert!(evs.iter().any(|e| matches!(e, Event::KnowledgeUpdated { facts, .. } if *facts > 0)));
    let p = rt.profile().unwrap();
    assert!(p.genres.iter().any(|g| g == "mmorpg"), "{:?}", p.genres);
    assert_ne!(p.visual_identity.hat, Hat::SyrupCap, "a new game gets a hat for its genres");
    assert_eq!(p.current_version.as_deref(), Some("1.3.1"));
    // Kept for next time.
    let saved: KnowledgeGraph = rt.memory.load(&p.game_id, "knowledge.json").unwrap();
    assert_eq!(saved.facts.len(), g.facts.len());
}

#[test]
fn feedback_reaches_the_coach_and_the_player_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = runtime(dir.path());
    let mut game = Session::of(GameKind::Dungeon, 4, 4.0, None);
    let mut said = None;
    for _ in 0..400 {
        let syrup_capture::Capture::Frame(f) = syrup_capture::FrameSource::next(&mut game).unwrap() else { break };
        let step = rt.on_frame(&f);
        if let Some(a) = step.shown.first() {
            said = Some(a.clone());
            break;
        }
    }
    let a = said.expect("Syrup said something");
    rt.commands()
        .send(Command::Feedback { advice_id: a.id, topic: a.topic.clone(), kind: FeedbackKind::StopSuggesting })
        .unwrap();
    let syrup_capture::Capture::Frame(f) = syrup_capture::FrameSource::next(&mut game).unwrap() else { panic!() };
    rt.on_frame(&f);
    let id = rt.identity().unwrap().game_id.clone();
    assert!(rt.player().games[&id].muted_topics.contains(&a.topic));
    assert!(
        events(&rt).iter().any(|e| matches!(e, Event::FeedbackReceived { feedback } if feedback.advice_id == a.id))
    );
    // The overlay drops a line the player silenced.
    assert!(rt.view.line.as_ref().is_none_or(|l| l.advice_id != a.id));
}

fn http(addr: std::net::SocketAddr, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    let host = format!("127.0.0.1:{}", addr.port());
    write!(s, "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()
}

#[test]
fn the_devtools_page_watches_and_steers_a_running_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut rt = runtime(dir.path());
    let server = Devtools::start(0, rt.shared(), rt.bus.clone(), rt.commands()).unwrap();
    let mut game = Session::of(GameKind::Cards, 1, 4.0, None);
    let mut next = |rt: &mut Runtime| {
        let syrup_capture::Capture::Frame(f) = syrup_capture::FrameSource::next(&mut game).unwrap() else { panic!() };
        rt.on_frame(&f);
    };
    for _ in 0..40 {
        next(&mut rt);
    }
    let snap: Snapshot = serde_json::from_str(&http(server.addr, "GET", "/api/snapshot", "")).unwrap();
    assert_eq!(snap.game.unwrap().title, "High Card Duel");
    let evs: Vec<syrup_core::EventRecord> =
        serde_json::from_str(&http(server.addr, "GET", "/api/events?since=0", "")).unwrap();
    assert!(evs.iter().any(|r| r.event.name() == "ui_element_discovered"));
    // "Analysis mode", and "this game is Balatro" (it is not, but the player decides).
    http(server.addr, "POST", "/api/mode", r#"{"mode":"analysis"}"#);
    http(server.addr, "POST", "/api/confirm", r#"{"title":"Balatro"}"#);
    for _ in 0..8 {
        next(&mut rt);
    }
    assert_eq!(rt.view.mode, OverlayMode::Analysis);
    assert!(rt.view.analysis.iter().any(|l| l.starts_with("game:")), "{:?}", rt.view.analysis);
    let id = rt.identity().unwrap();
    assert_eq!((id.game_id.as_str(), id.confirmed), ("balatro", true), "{id:?}");
}

#[test]
fn titles_name_catalogued_games() {
    assert_eq!(game_for_title("maplestory"), ("maplestory".to_string(), "MapleStory".to_string()));
    assert_eq!(game_for_title("Sky Meadow Online").0, "sky-meadow-online");
}
