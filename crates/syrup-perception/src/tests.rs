//! Perception measured against the synthetic games' ground truth.

use std::sync::Arc;

use syrup_capture::{Capture, FrameSource};
use syrup_core::{Observation, Rect, SceneKind, UiKind};
use syrup_testgames::{GameKind, Session, Truth};

use super::*;
use crate::text::{NoOcr, TesseractOcr};

/// Every analysed frame of `seconds` of a game, with the truth for it.
pub(crate) fn run(kind: GameKind, seconds: f32, ocr: bool) -> Vec<(Truth, Observation)> {
    let engine: Arc<dyn OcrEngine> =
        if ocr && TesseractOcr::available() { Arc::new(TesseractOcr) } else { Arc::new(NoOcr) };
    let cfg =
        PerceptionConfig { text: TextReaderConfig { full_every_ms: 4000, ..Default::default() }, ..Default::default() };
    let mut analyzer = SceneAnalyzer::new(cfg, engine);
    let mut sampler = FrameSampler::new(SamplerConfig::default());
    let mut session = Session::of(kind, 11, 10.0, Some(seconds));
    let log = session.truth();
    let mut out = Vec::new();
    while let Capture::Frame(f) = session.next().unwrap() {
        if !sampler.decide(&f).analyse {
            continue;
        }
        let obs = analyzer.analyze(&f);
        let truth = log.lock().unwrap().at(f.timestamp_ms).cloned().unwrap();
        out.push((truth, obs));
    }
    out
}

fn bar_near(obs: &Observation, rect: Rect) -> Option<(&syrup_core::UiRegion, f32)> {
    obs.ui_regions
        .iter()
        .filter_map(|r| match r.kind {
            UiKind::Bar { fill, .. } if r.rect.iou(&rect) > 0.5 => Some((r, fill)),
            _ => None,
        })
        .max_by(|a, b| a.0.rect.iou(&rect).total_cmp(&b.0.rect.iou(&rect)))
}

/// For one bar of the truth: in how many gameplay frames it was found, and the mean fill error.
fn bar_score(frames: &[(Truth, Observation)], name: &str, after_ms: u64) -> (usize, usize, f32) {
    let (mut found, mut total, mut err) = (0, 0, 0.0f32);
    for (t, o) in frames.iter().filter(|(t, _)| t.t_ms >= after_ms && t.scene == SceneKind::Gameplay) {
        let Some(e) = t.element(name) else { continue };
        total += 1;
        if let Some((r, fill)) = bar_near(o, e.rect) {
            found += 1;
            err += (fill - e.fraction().unwrap() as f32).abs();
            if std::env::var("SYRUP_DEBUG_BARS").is_ok() && (fill - e.fraction().unwrap() as f32).abs() > 0.04 {
                eprintln!("{} ms {name}: truth {:.2} seen {fill:.2} at {:?}", t.t_ms, e.fraction().unwrap(), r.rect);
            }
        }
    }
    (found, total, if found > 0 { err / found as f32 } else { 1.0 })
}

#[test]
fn the_scroller_hud_is_found_and_measured() {
    let frames = run(GameKind::Scroller, 30.0, false);
    for name in ["hp", "mp", "exp"] {
        let (found, total, err) = bar_score(&frames, name, 10_000);
        assert!(total > 10, "{name}: only {total} frames");
        assert!(found as f32 >= total as f32 * 0.85, "{name}: found in {found}/{total}");
        assert!(err < 0.05, "{name}: mean fill error {err}");
    }
    // The minimap is interface in the top left corner.
    let last = &frames.last().unwrap().1;
    let minimap = Rect::new(8, 8, 230, 118);
    assert!(last.ui_regions.iter().any(|r| r.rect.iou(&minimap) > 0.4), "{}", explain(last));
    // The scene is gameplay once playing.
    let playing: Vec<_> = frames.iter().filter(|(t, _)| t.t_ms > 8000).collect();
    let right = playing.iter().filter(|(_, o)| o.scene.kind == SceneKind::Gameplay).count();
    assert!(right as f32 >= playing.len() as f32 * 0.8, "gameplay in {right}/{}", playing.len());
}

#[test]
fn the_dungeon_health_bar_and_minimap() {
    let frames = run(GameKind::Dungeon, 40.0, false);
    let (found, total, err) = bar_score(&frames, "health", 12_000);
    assert!(total > 10);
    assert!(found as f32 >= total as f32 * 0.8, "health found in {found}/{total}");
    assert!(err < 0.05, "fill error {err}");
    let last = &frames.last().unwrap().1;
    let minimap = Rect::new(800, 12, 148, 148);
    assert!(last.ui_regions.iter().any(|r| r.rect.iou(&minimap) > 0.4), "{}", explain(last));
}

#[test]
fn the_card_game_timer_bar() {
    let frames = run(GameKind::Cards, 30.0, false);
    let (found, total, _) = bar_score(&frames, "timer_bar", 8000);
    assert!(total > 5);
    assert!(found as f32 >= total as f32 * 0.6, "timer bar found in {found}/{total}");
}

#[test]
fn deaths_are_seen_without_reading_a_word() {
    // No OCR: the defeat screen must be recognised from how it looks.
    let frames = run(GameKind::Dungeon, 70.0, false);
    let dead: Vec<_> = frames.iter().filter(|(t, _)| t.scene == SceneKind::Defeat).collect();
    assert!(!dead.is_empty(), "the session should include a death");
    let seen = dead.iter().filter(|(_, o)| o.scene.kind == SceneKind::Defeat).count();
    assert!(seen * 2 >= dead.len(), "defeat recognised in {seen}/{} frames", dead.len());
}

#[test]
fn with_ocr_the_words_decide_menus_and_deaths() {
    if !TesseractOcr::available() {
        eprintln!("tesseract not installed: skipped");
        return;
    }
    let frames = run(GameKind::Dungeon, 70.0, true);
    let menu: Vec<_> = frames.iter().filter(|(t, _)| t.scene == SceneKind::Menu && t.t_ms > 1000).collect();
    assert!(menu.iter().any(|(_, o)| o.scene.kind == SceneKind::Menu), "menu never recognised");
    let dead: Vec<_> = frames.iter().filter(|(t, _)| t.scene == SceneKind::Defeat).collect();
    let seen = dead.iter().filter(|(_, o)| o.scene.kind == SceneKind::Defeat).count();
    assert!(seen * 3 >= dead.len() * 2, "defeat in {seen}/{}", dead.len());
    let texts: Vec<String> = frames.iter().flat_map(|(_, o)| o.text.iter().map(|t| t.text.to_uppercase())).collect();
    assert!(texts.iter().any(|t| t.contains("AMMO")), "AMMO never read");
}

#[test]
#[ignore]
fn debug_dungeon_death_metrics() {
    for (t, o) in run(GameKind::Dungeon, 42.0, false).iter().filter(|(t, _)| t.t_ms > 30_000) {
        eprintln!(
            "{:>6} truth={:<8} seen={:<8} sat={:.3} red={:.3} change={:.3} {}",
            t.t_ms,
            t.scene.word(),
            o.scene.kind.word(),
            o.metrics.saturation,
            o.metrics.red_tint,
            o.metrics.change,
            o.scene.reason
        );
    }
}
