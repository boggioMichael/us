//! Runs perception on a test game or a recording and saves annotated frames:
//! `cargo run -p syrup-perception --example perceive -- <dungeon|scroller|cards|path> <seconds> <out-dir> [ocr]`

use std::sync::Arc;

use syrup_capture::{Capture, FrameSource};
use syrup_perception::text::{NoOcr, TesseractOcr};
use syrup_perception::{FrameSampler, OcrEngine, PerceptionConfig, SamplerConfig, SceneAnalyzer, annotate, explain};
use syrup_testgames::{GameKind, Session};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let what = args.first().cloned().unwrap_or_else(|| "scroller".into());
    let seconds: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let out = args.get(2).cloned().unwrap_or_else(|| "perceive-out".into());
    let ocr = args.get(3).is_some_and(|s| s == "ocr");
    std::fs::create_dir_all(&out).unwrap();
    let mut source: Box<dyn FrameSource> = match GameKind::parse(&what) {
        Some(kind) => Box::new(Session::of(kind, 11, 10.0, Some(seconds))),
        None => {
            let src = syrup_capture::VideoSource::open_at(
                std::path::Path::new(&what),
                Some(5.0),
                Some(1280),
                0.0,
                Some(seconds),
            )
            .unwrap();
            Box::new(src)
        }
    };
    let engine: Arc<dyn OcrEngine> =
        if ocr && TesseractOcr::available() { Arc::new(TesseractOcr) } else { Arc::new(NoOcr) };
    let mut analyzer = SceneAnalyzer::new(PerceptionConfig::default(), engine);
    let mut sampler = FrameSampler::new(SamplerConfig::default());
    let mut next_dump = 0u64;
    while let Ok(Capture::Frame(f)) = source.next() {
        if !sampler.decide(&f).analyse {
            continue;
        }
        let obs = analyzer.analyze(&f);
        if f.timestamp_ms >= next_dump {
            next_dump = f.timestamp_ms + 2500;
            print!("{}", explain(&obs));
            annotate(&f.image, &obs).save(format!("{out}/{:06}.png", f.timestamp_ms)).unwrap();
        }
    }
}
