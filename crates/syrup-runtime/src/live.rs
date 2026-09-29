//! The frame loop: frames from any source through the runtime, and Syrup
//! shown to the player.
//!
//! Live sources run in real time. Recordings run as fast as they decode,
//! or at their own speed with `realtime` (a replay to watch). On Windows the
//! overlay is a see-through window over the game; anywhere, pictures of it
//! can be saved whenever Syrup says something, for tests and evaluation.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use syrup_capture::{Capture, CaptureError, FrameSource};
use syrup_core::Frame;
use syrup_memory::SessionSummary;
use syrup_ui::{OverlayMode, Voice};

use crate::{Runtime, Step};

#[derive(Debug, Clone, Default)]
pub struct LiveOptions {
    /// Stop after this many seconds of the source.
    pub seconds: Option<f64>,
    /// Play recordings at their own speed.
    pub realtime: bool,
    /// Show the overlay window (Windows).
    pub overlay: bool,
    /// Speak shown lines (Windows).
    pub voice: bool,
    /// Save a picture of the overlay here each time Syrup says something, and at the end.
    pub overlay_dir: Option<PathBuf>,
    /// Print shown lines.
    pub print: bool,
    /// Keep the post-game card up this long at the end (overlay window only).
    pub linger: Duration,
    /// Set to stop (Ctrl+C, a test).
    pub stop: Option<Arc<AtomicBool>>,
}

/// What a run did.
#[derive(Debug, Clone)]
pub struct LiveReport {
    pub frames: u64,
    pub analysed: u64,
    /// `(source ms, line)` for everything Syrup said.
    pub said: Vec<(u64, String)>,
    /// Why the source had nothing, when it said so.
    pub waits: Vec<String>,
    pub overlay_pictures: Vec<PathBuf>,
    pub wall_s: f64,
    pub summary: SessionSummary,
}

/// Where Syrup is shown.
struct Stage {
    #[cfg(windows)]
    window: Option<syrup_ui::win::OverlayWindow>,
    last_paint: Option<Instant>,
    last_line: Option<u64>,
    last_mode: Option<OverlayMode>,
    anchor: Option<syrup_core::Rect>,
}

impl Stage {
    fn new(opts: &LiveOptions) -> Stage {
        #[cfg(windows)]
        let window = if opts.overlay {
            match syrup_ui::win::OverlayWindow::new() {
                Ok(w) => {
                    if !w.hidden_from_capture {
                        eprintln!(
                            "note: this Windows version cannot keep the overlay out of captures; Syrup ignores its own card instead"
                        );
                    }
                    Some(w)
                }
                Err(e) => {
                    eprintln!("could not open the overlay window: {e}");
                    None
                }
            }
        } else {
            None
        };
        #[cfg(not(windows))]
        let _ = opts;
        Stage {
            #[cfg(windows)]
            window,
            last_paint: None,
            last_line: None,
            last_mode: None,
            anchor: None,
        }
    }

    fn has_window(&self) -> bool {
        #[cfg(windows)]
        {
            self.window.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Repaints the window when something changed, or often enough to animate.
    fn show(&mut self, rt: &Runtime, t_ms: u64, force: bool) {
        if !self.has_window() {
            return;
        }
        let animate = rt.view.speaking && !rt.view.reduced_motion;
        let every = if animate { 80 } else { 400 };
        if !force && self.last_paint.is_some_and(|t| t.elapsed().as_millis() < every) {
            return;
        }
        self.last_paint = Some(Instant::now());
        #[cfg(windows)]
        if let Some(w) = self.window.as_mut() {
            let painted = syrup_ui::paint(&rt.view, t_ms);
            let at = syrup_ui::win::OverlayWindow::place(self.anchor, painted.image.width());
            if let Err(e) = w.show(painted, at) {
                eprintln!("overlay: {e}");
            }
        }
        #[cfg(not(windows))]
        let _ = t_ms;
    }

    fn pump(&mut self, rt: &mut Runtime) {
        #[cfg(windows)]
        if let Some(w) = self.window.as_mut() {
            w.pump();
            for action in w.take_actions() {
                rt.on_action(action);
                self.last_paint = None;
            }
        }
        #[cfg(not(windows))]
        let _ = rt;
    }

    /// Waits `d`, keeping the window responsive.
    fn wait(&mut self, rt: &mut Runtime, d: Duration) {
        let until = Instant::now() + d;
        loop {
            self.pump(rt);
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            std::thread::sleep(left.min(Duration::from_millis(if self.has_window() { 15 } else { 200 })));
        }
    }
}

fn save_picture(rt: &Runtime, dir: &std::path::Path, t_ms: u64, what: &str) -> Option<PathBuf> {
    if rt.view.mode == OverlayMode::Hidden {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    let painted = syrup_ui::paint(&rt.view, t_ms);
    let path = dir.join(format!("overlay-{t_ms:08}-{what}.png"));
    painted.image.save(&path).ok()?;
    Some(path)
}

/// Runs `source` through `rt` until it ends, `opts.seconds` pass, or `opts.stop` is set.
pub fn run(rt: &mut Runtime, source: &mut dyn FrameSource, opts: &LiveOptions) -> Result<LiveReport, CaptureError> {
    run_with(rt, source, opts, |_, _, _| {})
}

/// [`run`], calling `observe` after every frame (to print, measure, compare with truth).
pub fn run_with(
    rt: &mut Runtime,
    source: &mut dyn FrameSource,
    opts: &LiveOptions,
    mut observe: impl FnMut(&Runtime, &Frame, &Step),
) -> Result<LiveReport, CaptureError> {
    let started = Instant::now();
    let live = source.is_live();
    let interval = Duration::from_secs_f32(1.0 / source.nominal_fps().clamp(0.5, 60.0));
    let voice = Voice::new(opts.voice);
    crate::trace("overlay window");
    let mut stage = Stage::new(opts);
    crate::trace("loop");
    let mut report = LiveReport {
        frames: 0,
        analysed: 0,
        said: Vec::new(),
        waits: Vec::new(),
        overlay_pictures: Vec::new(),
        wall_s: 0.0,
        summary: SessionSummary::default(),
    };
    let mut first_ts: Option<u64> = None;
    let mut last_frame: Option<Frame> = None;
    loop {
        if opts.stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed)) {
            break;
        }
        let tick = Instant::now();
        crate::trace("capture");
        match source.next() {
            Ok(Capture::Frame(frame)) => {
                let ts = frame.timestamp_ms;
                let first = *first_ts.get_or_insert(ts);
                if opts.seconds.is_some_and(|s| ts.saturating_sub(first) as f64 / 1000.0 > s) {
                    break;
                }
                report.frames += 1;
                stage.anchor =
                    Some(syrup_core::Rect::new(frame.origin.0, frame.origin.1, frame.width(), frame.height()));
                crate::trace("frame");
                let step = rt.on_frame(&frame);
                crate::trace("frame done");
                if step.analysed {
                    report.analysed += 1;
                }
                observe(rt, &frame, &step);
                for a in &step.shown {
                    if opts.print {
                        println!("[{:>7.1}s] Syrup: {}", ts as f64 / 1000.0, a.text);
                    }
                    voice.say(&a.text);
                    report.said.push((ts, a.text.clone()));
                }
                let line = rt.view.line.as_ref().map(|l| l.advice_id);
                let changed = line != stage.last_line || Some(rt.view.mode) != stage.last_mode;
                if changed {
                    if let (Some(dir), Some(id)) = (&opts.overlay_dir, line)
                        && line != stage.last_line
                        && let Some(p) = save_picture(rt, dir, ts, &format!("line{id}"))
                    {
                        report.overlay_pictures.push(p);
                    }
                    stage.last_line = line;
                    stage.last_mode = Some(rt.view.mode);
                }
                crate::trace("overlay");
                stage.show(rt, ts, changed);
                stage.pump(rt);
                if live {
                    let spent = tick.elapsed();
                    if spent < interval {
                        stage.wait(rt, interval - spent);
                    }
                } else if opts.realtime {
                    let due = Duration::from_millis(ts.saturating_sub(first));
                    let now = started.elapsed();
                    if due > now {
                        stage.wait(rt, due - now);
                    }
                }
                last_frame = Some(frame);
            }
            Ok(Capture::Waiting(why)) => {
                if report.waits.last() != Some(&why) {
                    if opts.print {
                        eprintln!("waiting: {why}");
                    }
                    report.waits.push(why);
                }
                stage.wait(rt, Duration::from_millis(250));
            }
            Ok(Capture::Ended) => break,
            Err(e) => {
                if report.frames == 0 {
                    return Err(e);
                }
                eprintln!("capture stopped: {e}");
                break;
            }
        }
    }
    report.summary = rt.finish();
    let t = last_frame.as_ref().map(|f| f.timestamp_ms).unwrap_or(0);
    if let Some(dir) = &opts.overlay_dir
        && let Some(p) = save_picture(rt, dir, t, "summary")
    {
        report.overlay_pictures.push(p);
    }
    if stage.has_window() && !opts.linger.is_zero() {
        stage.show(rt, t, true);
        stage.wait(rt, opts.linger);
    }
    report.wall_s = started.elapsed().as_secs_f64();
    Ok(report)
}
