//! `syrup-testgame`: one of Syrup's synthetic games in a window, to try
//! `syrup live` without a real game (and to test live capture in CI).
//!
//! ```text
//! syrup-testgame [dungeon|scroller|cards] [--seed N] [--fps N] [--seconds S]
//! ```
//!
//! Without a game on the command line the program's own name picks it:
//! copied to `dungeon3d.exe`, `skymeadow.exe` or `highcard.exe` it runs that
//! game, so the window list shows it the way it would show a real game. Esc
//! closes it. The game plays itself; the window takes no input.

use std::process::ExitCode;
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};
use syrup_capture::FrameSource;
use syrup_testgames::{GameKind, Session};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("syrup-testgame: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut kind = None;
    let (mut seed, mut fps, mut seconds) = (1u64, 30.0f32, None::<f64>);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--seed" => seed = value("--seed")?.parse().map_err(|_| "--seed takes a number")?,
            "--fps" => fps = value("--fps")?.parse().map_err(|_| "--fps takes a number")?,
            "--seconds" => seconds = Some(value("--seconds")?.parse().map_err(|_| "--seconds takes a number")?),
            "-h" | "--help" => {
                println!("syrup-testgame [dungeon|scroller|cards] [--seed N] [--fps N] [--seconds S]");
                return Ok(());
            }
            other => {
                kind = Some(
                    GameKind::parse(other)
                        .ok_or_else(|| format!("no test game called \"{other}\" (dungeon, scroller, cards)"))?,
                )
            }
        }
    }
    let kind = kind
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            GameKind::parse(exe.file_stem()?.to_str()?)
        })
        .unwrap_or(GameKind::Dungeon);
    let fps = fps.clamp(1.0, 60.0);
    let mut session = Session::of(kind, seed, fps, None);
    let title = session.info().window_title.clone().unwrap_or_else(|| "Syrup test game".into());
    let (mut img, _) = session.step_image();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut window =
        Window::new(&title, w, h, WindowOptions::default()).map_err(|e| format!("could not open a window: {e}"))?;
    window.set_target_fps(fps.round() as usize);
    let started = Instant::now();
    let mut buf = vec![0u32; w * h];
    while window.is_open() && !window.is_key_down(Key::Escape) {
        for (dst, p) in buf.iter_mut().zip(img.pixels()) {
            *dst = (p.0[0] as u32) << 16 | (p.0[1] as u32) << 8 | p.0[2] as u32;
        }
        window.update_with_buffer(&buf, w, h).map_err(|e| format!("could not draw: {e}"))?;
        if seconds.is_some_and(|s| started.elapsed().as_secs_f64() >= s) {
            break;
        }
        img = session.step_image().0;
    }
    Ok(())
}
