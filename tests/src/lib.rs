//! End-to-end tests of Syrup Universal (in `tests/`): three very different
//! synthetic games (a first-person dungeon, an MMO-style side-scroller and a
//! card game) through the whole runtime, measured against each game's truth,
//! in one data folder, twice. This library holds what the tests share.

use std::path::Path;
use std::sync::Arc;

use syrup_capture::FrameSource;
use syrup_core::SceneKind;
use syrup_perception::OcrEngine;
use syrup_runtime::live::{LiveOptions, LiveReport, run_with};
use syrup_runtime::{Runtime, RuntimeConfig};
use syrup_testgames::score::{Happened, Score};
use syrup_testgames::{GameKind, Session};

/// OCR if this computer has an engine (Tesseract, or Windows' own); none
/// otherwise. `SYRUP_OCR=none|tesseract|windows` chooses.
pub fn ocr() -> Arc<dyn OcrEngine> {
    std::env::var("SYRUP_OCR")
        .ok()
        .and_then(|name| syrup_perception::engine_named(&name))
        .unwrap_or_else(syrup_perception::best_engine)
}

pub fn has_ocr() -> bool {
    ocr().is_available()
}

/// A runtime set up the way `syrup replay` and `syrup simulate` set it up.
pub fn runtime(data: &Path, ocr: Arc<dyn OcrEngine>, fixtures: Option<&Path>) -> Runtime {
    let mut cfg = RuntimeConfig::new(data, ocr);
    cfg.sampler.budget_ms = f32::INFINITY;
    if let Some(dir) = fixtures {
        cfg.research = true;
        cfg.fetcher = Some(Arc::new(syrup_knowledge::Fixtures::new(dir)));
    }
    Runtime::new(cfg).expect("a data folder")
}

/// What one session did, and how it compares with the truth.
pub struct Played {
    pub report: LiveReport,
    pub score: Score,
    pub happened: Happened,
}

/// Plays `seconds` of a game through `rt`, scoring every analysed frame.
pub fn play(rt: &mut Runtime, kind: GameKind, seed: u64, seconds: f32) -> Played {
    let mut game = Session::of(kind, seed, 4.0, Some(seconds));
    let truth = game.truth();
    let mut score = Score::default();
    let report = run_with(rt, &mut game as &mut dyn FrameSource, &LiveOptions::default(), |rt, frame, step| {
        if !step.analysed {
            return;
        }
        let log = truth.lock().unwrap();
        if let Some(t) = log.at(frame.timestamp_ms) {
            let scene = rt.last_observation().map(|o| o.scene.kind).unwrap_or(SceneKind::Unknown);
            score.observe(t, scene, |name| rt.state().concept(name).map(|c| c.fraction()));
        }
    })
    .expect("the game runs");
    let happened = Happened::from(&truth.lock().unwrap());
    Played { report, score, happened }
}

impl Played {
    /// One line per measure, for the test log.
    pub fn describe(&self, name: &str) -> String {
        let s = &self.report.summary;
        let mut out = format!(
            "{name}: {} analysed · scene {:.0}% · defeats {}/{} · wins {}/{} · level-ups {}/{} · said {}",
            self.report.analysed,
            self.score.scene_accuracy() * 100.0,
            s.deaths,
            self.happened.defeats,
            s.victories,
            self.happened.wins,
            s.level_ups,
            self.happened.level_ups,
            s.advice_shown
        );
        for (c, v) in &self.score.concepts {
            out.push_str(&format!(
                " · {c} {:.0}% known, error {}",
                v.coverage() * 100.0,
                v.mean_error().map(|e| format!("{e:.3}")).unwrap_or_else(|| "–".into())
            ));
        }
        out
    }
}
