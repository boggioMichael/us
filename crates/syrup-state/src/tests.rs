//! The state engine on the synthetic games: concepts found from evidence,
//! deaths caught, against ground truth.

use std::sync::Arc;

use syrup_capture::{Capture, FrameSource};
use syrup_core::{GameState, Rect, TransitionKind};
use syrup_perception::text::{NoOcr, TesseractOcr};
use syrup_perception::{FrameSampler, OcrEngine, PerceptionConfig, SamplerConfig, SceneAnalyzer, TextReaderConfig};
use syrup_testgames::{GameKind, Session, TruthEventKind, TruthLog};

use super::*;

struct Run {
    states: Vec<GameState>,
    transitions: Vec<Transition>,
    learned: Vec<ConceptLearned>,
    truth: TruthLog,
}

fn run(kind: GameKind, seconds: f32, ocr: bool) -> Run {
    let engine: Arc<dyn OcrEngine> =
        if ocr && TesseractOcr::available() { Arc::new(TesseractOcr) } else { Arc::new(NoOcr) };
    let cfg =
        PerceptionConfig { text: TextReaderConfig { full_every_ms: 4000, ..Default::default() }, ..Default::default() };
    let mut analyzer = SceneAnalyzer::new(cfg, engine);
    let mut sampler = FrameSampler::new(SamplerConfig::default());
    let mut session = Session::of(kind, 11, 10.0, Some(seconds));
    let log = session.truth();
    let mut engine = StateEngine::new();
    let (mut states, mut transitions, mut learned) = (Vec::new(), Vec::new(), Vec::new());
    while let Capture::Frame(f) = session.next().unwrap() {
        if !sampler.decide(&f).analyse {
            continue;
        }
        let obs = analyzer.analyze(&f);
        let up = engine.update(&obs);
        transitions.extend(up.transitions);
        learned.extend(up.learned);
        states.push(engine.state().clone());
    }
    let truth = log.lock().unwrap().clone();
    Run { states, transitions, learned, truth }
}

fn source_rect(state: &GameState, concept: &str, run: &Run) -> Option<Rect> {
    let _ = run;
    state.concept(concept).map(|c| c.source.clone()).and(None)
}

#[test]
fn health_is_found_in_the_dungeon_without_reading_a_word() {
    let r = run(GameKind::Dungeon, 75.0, false);
    let last = r.states.last().unwrap();
    let health =
        last.concept("health").unwrap_or_else(|| panic!("no health: {:?}", last.concepts.keys().collect::<Vec<_>>()));
    assert!(health.confidence.value() >= 0.5, "{health:?}");
    let _ = source_rect(last, "health", &r);
    // Its value follows the truth.
    let t = r.truth.at(last.timestamp_ms).unwrap();
    if let (Some(truth), Some(v)) = (t.element("health").and_then(|e| e.fraction()), health.fraction()) {
        assert!((truth - v).abs() < 0.1, "health {v} vs truth {truth}");
    }
    // Every death is noticed within two seconds.
    let deaths: Vec<u64> = r.truth.events.iter().filter(|e| e.kind == TruthEventKind::Died).map(|e| e.t_ms).collect();
    assert!(!deaths.is_empty());
    for d in &deaths {
        assert!(
            r.transitions.iter().any(|t| t.kind == TransitionKind::PlayerDied && t.ts_ms >= *d && t.ts_ms <= d + 2500),
            "death at {d} missed: {:?}",
            r.transitions.iter().filter(|t| t.kind == TransitionKind::PlayerDied).map(|t| t.ts_ms).collect::<Vec<_>>()
        );
    }
    assert!(r.learned.iter().any(|l| l.concept == "health" && l.kind == "bar"));
}

#[test]
fn the_scroller_has_health_and_experience() {
    let r = run(GameKind::Scroller, 70.0, false);
    let last = r.states.last().unwrap();
    let names: Vec<&String> = last.concepts.keys().collect();
    assert!(last.concept("experience").is_some(), "{names:?}");
    // Health: the red bar, which ran empty before the boss killed the player.
    let learned: Vec<(&str, &str)> = r.learned.iter().map(|l| (l.concept.as_str(), l.source.as_str())).collect();
    assert!(learned.iter().any(|(c, _)| *c == "health"), "{learned:?}");
}

#[test]
fn the_card_game_timer_bar_is_a_timer() {
    let r = run(GameKind::Cards, 60.0, false);
    let learned: Vec<(&str, &str)> = r.learned.iter().map(|l| (l.concept.as_str(), l.source.as_str())).collect();
    assert!(learned.iter().any(|(c, _)| *c == "timer"), "{learned:?}");
}

#[test]
fn with_ocr_labels_name_things() {
    if !TesseractOcr::available() {
        eprintln!("tesseract not installed: skipped");
        return;
    }
    let r = run(GameKind::Scroller, 30.0, true);
    let last = r.states.last().unwrap();
    for c in ["health", "mana", "experience"] {
        let cv = last.concept(c).unwrap_or_else(|| panic!("no {c}: {:?}", last.concepts));
        assert!(cv.confidence.value() >= 0.6, "{c}: {cv:?}");
        assert!(cv.evidence.iter().any(|e| e.contains("labelled")), "{c}: {:?}", cv.evidence);
    }
    assert!(last.concept("level").is_some(), "{:?}", last.concepts.keys().collect::<Vec<_>>());
}

#[test]
#[ignore]
fn debug_scroller_ocr() {
    let engine: Arc<dyn OcrEngine> = Arc::new(TesseractOcr);
    let mut analyzer = SceneAnalyzer::new(PerceptionConfig::default(), engine);
    let mut sampler = FrameSampler::new(SamplerConfig::default());
    let mut session = Session::of(GameKind::Scroller, 11, 10.0, Some(30.0));
    let mut engine = StateEngine::new();
    let mut last_print = 0;
    while let Capture::Frame(f) = session.next().unwrap() {
        if !sampler.decide(&f).analyse {
            continue;
        }
        let obs = analyzer.analyze(&f);
        engine.update(&obs);
        if f.timestamp_ms >= last_print + 5000 {
            last_print = f.timestamp_ms;
            eprintln!("--- {} ms", f.timestamp_ms);
            for t in &obs.text {
                eprintln!("  text {:?} region={:?} fresh={}", t.text, t.region, t.fresh);
            }
            for (k, c) in &engine.state().concepts {
                eprintln!(
                    "  {k}: {} from {} ({:.2}) {:?}",
                    c.value.unwrap_or(-1.0),
                    c.source,
                    c.confidence.value(),
                    c.evidence
                );
            }
        }
    }
}
