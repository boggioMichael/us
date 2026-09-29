//! Prints a test game's truth events: `cargo run --example truth_events -- cards 90`.
use syrup_capture::{Capture, FrameSource};
use syrup_testgames::{GameKind, Session};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kind = args.first().and_then(|a| GameKind::parse(a)).unwrap_or(GameKind::Cards);
    let secs: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(90.0);
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut s = Session::of(kind, seed, 4.0, Some(secs));
    while let Ok(Capture::Frame(_)) = s.next() {}
    for e in &s.truth().lock().unwrap().events {
        println!("{:>7.1}s {:?}", e.t_ms as f64 / 1000.0, e.kind);
    }
}
