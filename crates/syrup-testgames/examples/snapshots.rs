//! Saves frames of each test game at a few moments: `cargo run -p syrup-testgames --example snapshots -- <dir>`.

use syrup_testgames::{GameKind, Session};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "snapshots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let moments = [2.0f32, 5.0, 12.0, 30.0, 50.0, 57.0, 65.0];
    for kind in GameKind::ALL {
        let mut s = Session::of(kind, 1, 10.0, None);
        let mut t = 0.0;
        for m in moments {
            let mut last = None;
            while t <= m {
                last = Some(s.step_image());
                t += 0.1;
            }
            if let Some((img, truth)) = last {
                let path = format!(
                    "{dir}/{}-{:03}s-{}.png",
                    kind.name(),
                    m as u32,
                    truth.scene.word()
                );
                img.save(&path).unwrap();
                println!("{path}");
            }
        }
    }
}
