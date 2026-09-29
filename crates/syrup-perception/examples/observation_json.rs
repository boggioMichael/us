//! Prints what perception saw on one frame of a test game, as JSON (the
//! observation IR): `cargo run -p syrup-perception --example observation_json -- scroller 20`

use std::sync::Arc;

use syrup_capture::{Capture, FrameSource};
use syrup_perception::{FrameSampler, OcrEngine, PerceptionConfig, SamplerConfig, SceneAnalyzer};
use syrup_testgames::{GameKind, Session};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kind = args.first().and_then(|a| GameKind::parse(a)).unwrap_or(GameKind::Scroller);
    let seconds: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let engine: Arc<dyn OcrEngine> = syrup_perception::best_engine();
    let mut analyzer = SceneAnalyzer::new(PerceptionConfig::default(), engine);
    let mut sampler = FrameSampler::new(SamplerConfig { budget_ms: f32::INFINITY, ..Default::default() });
    let mut game = Session::of(kind, 1, 4.0, Some(seconds));
    let mut last = None;
    while let Ok(Capture::Frame(f)) = game.next() {
        if sampler.decide(&f).analyse {
            last = Some(analyzer.analyze(&f));
        }
    }
    println!("{}", serde_json::to_string_pretty(&last.expect("a frame")).unwrap());
}
