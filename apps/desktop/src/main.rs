//! `syrup`: Syrup Universal, the game companion that learns the game with you.
//!
//! ```text
//! syrup live [--window TITLE | --exe NAME | --screen]  watch a game and coach (Windows)
//! syrup replay VIDEO|FOLDER|IMAGE [--explain]          the same pipeline on a recording
//! syrup simulate dungeon|scroller|cards [--truth]      on a synthetic game, measured against its truth
//! syrup research "Game title" [--topic T]              look a game (or one thing in it) up
//! syrup profiles [GAME]                                what Syrup has learned
//! syrup forget GAME | --player | --recordings | --sessions | --everything
//! syrup avatar [--out DIR]                             Syrup's hats, faces and overlay, as pictures
//! syrup windows                                        the windows Syrup could watch (Windows)
//! ```
//!
//! Syrup only ever reads pixels. It never sends input to a game, never reads
//! or writes a game's memory, and never touches its files or its network.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use syrup_capture::{FrameSource, WindowSelector};
use syrup_coach::SpoilerPolicy;
use syrup_core::paths::DataDir;
use syrup_core::{Event, Expression, Hat};
use syrup_knowledge::{Cached, Curl, Fetcher, Fixtures, KnowledgeGraph, ResearchAgent, ResearchQuestion};
use syrup_memory::MemoryStore;
use syrup_runtime::devtools::Devtools;
use syrup_runtime::live::{LiveOptions, LiveReport, run_with};
use syrup_runtime::{Runtime, RuntimeConfig, concept_text, game_for_title};
use syrup_testgames::{GameKind, Session, TruthEventKind};
use syrup_ui::{Line, OverlayMode, OverlayView};

fn run_args(c: Command) -> Command {
    c.arg(
        Arg::new("data-dir")
            .long("data-dir")
            .value_name("DIR")
            .value_parser(value_parser!(PathBuf))
            .help("Where Syrup keeps what it learns [default: $SYRUP_DATA_DIR or the app data folder]"),
    )
    .arg(
        Arg::new("player").long("player").value_name("NAME").default_value("default").help("Whose player model to use"),
    )
    .arg(
        Arg::new("research")
            .long("research")
            .action(ArgAction::SetTrue)
            .help("Look the game up online (only search terms leave the computer)"),
    )
    .arg(
        Arg::new("no-research")
            .long("no-research")
            .action(ArgAction::SetTrue)
            .conflicts_with("research")
            .help("Never go online"),
    )
    .arg(
        Arg::new("fixtures")
            .long("fixtures")
            .value_name("DIR")
            .value_parser(value_parser!(PathBuf))
            .help("Research from recorded responses in DIR instead of the web"),
    )
    .arg(
        Arg::new("mode")
            .long("mode")
            .value_name("MODE")
            .default_value("normal")
            .help("Overlay: hidden, minimal, normal, analysis"),
    )
    .arg(
        Arg::new("spoilers")
            .long("spoilers")
            .value_name("LEVEL")
            .default_value("normal")
            .help("none, hints_only, normal, full_information"),
    )
    .arg(
        Arg::new("quiet-learning")
            .long("quiet-learning")
            .action(ArgAction::SetTrue)
            .help("Don't say what Syrup is learning (\"that red bar is health\")"),
    )
    .arg(
        Arg::new("record")
            .long("record")
            .action(ArgAction::SetTrue)
            .help("Keep a frame a second under recordings/ in the data folder"),
    )
    .arg(
        Arg::new("seconds")
            .long("seconds")
            .value_name("S")
            .value_parser(value_parser!(f64))
            .help("Stop after this many seconds"),
    )
    .arg(
        Arg::new("ocr")
            .long("ocr")
            .value_name("ENGINE")
            .default_value("auto")
            .help("Reading text: auto, windows, tesseract, none"),
    )
    .arg(
        Arg::new("devtools")
            .long("devtools")
            .value_name("PORT")
            .num_args(0..=1)
            .default_missing_value("7777")
            .value_parser(value_parser!(u16))
            .help("Serve the devtools page on 127.0.0.1 [default port: 7777]"),
    )
    .arg(
        Arg::new("overlay-dir")
            .long("overlay-dir")
            .value_name("DIR")
            .value_parser(value_parser!(PathBuf))
            .help("Save a picture of the overlay each time Syrup speaks"),
    )
    .arg(Arg::new("reduced-motion").long("reduced-motion").action(ArgAction::SetTrue).help("No blinking or bobbing"))
    .arg(Arg::new("game").long("game").value_name("TITLE").help("Say which game this is"))
    .arg(
        Arg::new("plugins")
            .long("plugins")
            .value_name("DIR")
            .value_parser(value_parser!(PathBuf))
            .help("Load data plugins (folders with a plugin.json) from DIR"),
    )
    .arg(Arg::new("json").long("json").action(ArgAction::SetTrue).help("Print the session summary as JSON"))
    .arg(Arg::new("quiet").long("quiet").short('q').action(ArgAction::SetTrue).help("Don't print Syrup's lines"))
    .arg(Arg::new("explain").long("explain").action(ArgAction::SetTrue).help("Print what Syrup notices as it happens"))
}

fn data_arg(c: Command) -> Command {
    c.arg(
        Arg::new("data-dir")
            .long("data-dir")
            .value_name("DIR")
            .value_parser(value_parser!(PathBuf))
            .help("Where Syrup keeps what it learns [default: $SYRUP_DATA_DIR or the app data folder]"),
    )
}

fn cli() -> Command {
    Command::new("syrup")
        .about("Syrup learns the game with you.")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            run_args(Command::new("live").about("Watch a game live and coach (Windows)"))
                .arg(
                    Arg::new("window").long("window").value_name("TITLE").help("The window whose title contains TITLE"),
                )
                .arg(
                    Arg::new("exe")
                        .long("exe")
                        .value_name("NAME")
                        .conflicts_with("window")
                        .help("The window of this program (e.g. MapleStory.exe)"),
                )
                .arg(
                    Arg::new("screen")
                        .long("screen")
                        .action(ArgAction::SetTrue)
                        .conflicts_with_all(["window", "exe"])
                        .help("The whole screen"),
                )
                .arg(
                    Arg::new("fps")
                        .long("fps")
                        .value_parser(value_parser!(f32))
                        .default_value("8")
                        .help("Frames a second to capture"),
                )
                .arg(Arg::new("no-overlay").long("no-overlay").action(ArgAction::SetTrue).help("No overlay window"))
                .arg(Arg::new("no-voice").long("no-voice").action(ArgAction::SetTrue).help("Don't speak")),
        )
        .subcommand(
            run_args(
                Command::new("replay")
                    .about("Run a recording (a video, a folder of screenshots, an image) through Syrup"),
            )
            .arg(Arg::new("path").required(true).value_parser(value_parser!(PathBuf)))
            .arg(
                Arg::new("fps")
                    .long("fps")
                    .value_parser(value_parser!(f32))
                    .help("Frames a second to decode [default: the video's, at most 10]"),
            )
            .arg(
                Arg::new("start")
                    .long("start")
                    .value_name("S")
                    .value_parser(value_parser!(f64))
                    .help("Start this many seconds in"),
            )
            .arg(
                Arg::new("width")
                    .long("width")
                    .value_parser(value_parser!(u32))
                    .help("Scale frames down to at most this wide"),
            )
            .arg(
                Arg::new("title").long("title").help("The window title the recording had (a clue to which game it is)"),
            )
            .arg(
                Arg::new("realtime")
                    .long("realtime")
                    .action(ArgAction::SetTrue)
                    .help("Play at the recording's own speed"),
            ),
        )
        .subcommand(
            run_args(Command::new("simulate").about("Run one of the synthetic test games through Syrup"))
                .arg(Arg::new("which").value_name("GAME").required(true).help("dungeon, scroller or cards"))
                .arg(Arg::new("seed").long("seed").value_parser(value_parser!(u64)).default_value("1"))
                .arg(Arg::new("fps").long("fps").value_parser(value_parser!(f32)).default_value("4"))
                .arg(
                    Arg::new("truth")
                        .long("truth")
                        .action(ArgAction::SetTrue)
                        .help("Measure what Syrup saw against the game's truth"),
                ),
        )
        .subcommand(
            data_arg(
                Command::new("research").about("Look a game, or one thing in it, up (and remember what was found)"),
            )
            .arg(Arg::new("title").required(true).help("The game"))
            .arg(Arg::new("topic").long("topic").help("One thing in the game: a boss, an item, a mechanic"))
            .arg(
                Arg::new("fixtures")
                    .long("fixtures")
                    .value_name("DIR")
                    .value_parser(value_parser!(PathBuf))
                    .help("Recorded responses instead of the web"),
            )
            .arg(Arg::new("played").long("version-played").value_name("V").help("The version being played")),
        )
        .subcommand(
            data_arg(Command::new("profiles").about("What Syrup has learned about each game"))
                .arg(Arg::new("game").help("One game in detail")),
        )
        .subcommand(
            data_arg(
                Command::new("forget").about("Forget a game, the player model, recordings, sessions, or everything"),
            )
            .arg(Arg::new("game"))
            .arg(Arg::new("player").long("player").value_name("NAME").num_args(0..=1).default_missing_value("default"))
            .arg(Arg::new("recordings").long("recordings").action(ArgAction::SetTrue))
            .arg(Arg::new("sessions").long("sessions").action(ArgAction::SetTrue))
            .arg(Arg::new("everything").long("everything").action(ArgAction::SetTrue)),
        )
        .subcommand(
            Command::new("avatar")
                .about("Draw Syrup: every hat, every face, every overlay mode")
                .arg(Arg::new("out").long("out").value_parser(value_parser!(PathBuf)).default_value("syrup-avatar")),
        )
        .subcommand(
            Command::new("windows").about("List the windows Syrup could watch, and which one it would pick (Windows)"),
        )
}

fn main() -> ExitCode {
    let m = cli().get_matches();
    let result = match m.subcommand() {
        Some(("live", m)) => live(m),
        Some(("replay", m)) => replay(m),
        Some(("simulate", m)) => simulate(m),
        Some(("research", m)) => research(m),
        Some(("profiles", m)) => profiles(m),
        Some(("forget", m)) => forget(m),
        Some(("avatar", m)) => avatar(m),
        Some(("windows", _)) => windows(),
        _ => Err("unknown command".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("syrup: {e}");
            ExitCode::FAILURE
        }
    }
}

fn data_dir(m: &ArgMatches) -> PathBuf {
    m.get_one::<PathBuf>("data-dir").cloned().unwrap_or_else(DataDir::default_location)
}

/// The runtime's settings from the command line. `live`: a game running now
/// (research on unless turned off; text is read off the loop; analysis backs
/// off when it falls behind). Otherwise a recording: every sampled frame is
/// analysed however long it takes, so results do not depend on this
/// computer's speed.
fn config(m: &ArgMatches, live: bool) -> Result<RuntimeConfig, String> {
    let ocr_name = m.get_one::<String>("ocr").map(|s| s.as_str()).unwrap_or("auto");
    let ocr = syrup_perception::engine_named(ocr_name)
        .ok_or_else(|| format!("no OCR engine called \"{ocr_name}\" here (try auto, tesseract, windows or none)"))?;
    let mut cfg = RuntimeConfig::new(data_dir(m), ocr);
    cfg.perception.text.asynchronous = live;
    if !live {
        cfg.sampler.budget_ms = f32::INFINITY;
    }
    let research_by_default = live;
    cfg.player_id = m.get_one::<String>("player").cloned().unwrap_or_else(|| "default".into());
    let fixtures = m.get_one::<PathBuf>("fixtures");
    cfg.research = !m.get_flag("no-research") && (m.get_flag("research") || fixtures.is_some() || research_by_default);
    if let Some(dir) = fixtures {
        cfg.fetcher = Some(Arc::new(Fixtures::new(dir)));
    }
    let mode = m.get_one::<String>("mode").map(|s| s.as_str()).unwrap_or("normal");
    cfg.overlay_mode = OverlayMode::parse(mode).ok_or_else(|| format!("no overlay mode called \"{mode}\""))?;
    let spoilers = m.get_one::<String>("spoilers").map(|s| s.as_str()).unwrap_or("normal");
    cfg.coach.spoilers =
        SpoilerPolicy::parse(spoilers).ok_or_else(|| format!("no spoiler level called \"{spoilers}\""))?;
    cfg.coach.narrate_learning = !m.get_flag("quiet-learning");
    cfg.record = m.get_flag("record");
    cfg.reduced_motion = m.get_flag("reduced-motion");
    cfg.plugins_dir = m.get_one::<PathBuf>("plugins").cloned();
    cfg.confirm = m.get_one::<String>("game").map(|t| game_for_title(t));
    Ok(cfg)
}

/// Stops the run when the player presses Enter (a closed or missing stdin never stops it).
fn stop_on_enter(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut line = String::new();
        if let Ok(n) = std::io::stdin().lock().read_line(&mut line)
            && n > 0
        {
            stop.store(true, Ordering::Relaxed);
        }
    });
}

/// One line for the events worth printing with `--explain`.
fn describe(e: &Event) -> Option<(u64, String)> {
    Some(match e {
        Event::GameIdentified { ts_ms, identity } => (
            *ts_ms,
            format!(
                "game: {} ({}) — {}",
                identity.title,
                identity.confidence,
                identity.evidence.iter().map(|e| e.signal.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ),
        Event::GameUncertain { ts_ms, best: Some(b) } => {
            (*ts_ms, format!("not sure which game: maybe {} ({})", b.title, b.confidence))
        }
        Event::SceneChanged { ts_ms, from, to } => (*ts_ms, format!("scene: {} → {}", from.word(), to.word())),
        Event::UiElementDiscovered { ts_ms, region, kind, place } => {
            (*ts_ms, format!("found #{region}: a {kind} at the {place}"))
        }
        Event::ConceptLearned { ts_ms, concept, confidence, evidence, .. } => {
            (*ts_ms, format!("learned: {concept} ({confidence}) — {}", evidence.join("; ")))
        }
        Event::PlayerDied { ts_ms, context } => (*ts_ms, format!("the player died {context}")),
        Event::ObjectiveChanged { ts_ms, text } => (*ts_ms, format!("objective: {text}")),
        Event::ResearchRequested { ts_ms, question, .. } => (*ts_ms, format!("looking up: {question}")),
        Event::KnowledgeUpdated { ts_ms, question, facts, sources, .. } => {
            (*ts_ms, format!("found {facts} facts from {sources} sources: {question}"))
        }
        Event::ResearchFailed { ts_ms, question, reason, .. } => {
            (*ts_ms, format!("nothing found for {question}: {reason}"))
        }
        Event::PluginActivated { ts_ms, plugin, .. } => (*ts_ms, format!("plugin: {plugin}")),
        Event::AdviceSuppressed { ts_ms, topic, reason, .. } => (*ts_ms, format!("held back ({topic}): {reason}")),
        Event::Note { ts_ms, message } => (*ts_ms, message.clone()),
        _ => return None,
    })
}

fn state_line(rt: &Runtime) -> String {
    let mut parts = Vec::new();
    if let Some(o) = rt.last_observation() {
        parts.push(format!("{} ({})", o.scene.kind.word(), o.scene.confidence));
    }
    let mut concepts: Vec<_> = rt.state().concepts.values().filter(|c| c.confidence.at_least(0.5)).collect();
    concepts.sort_by(|a, b| a.name.cmp(&b.name));
    for c in concepts.into_iter().take(10) {
        parts.push(format!("{} {}", c.name.replace('_', " "), concept_text(c)));
    }
    parts.join(" · ")
}

/// Runs a source through a new runtime and prints what happened.
fn go(
    m: &ArgMatches,
    cfg: RuntimeConfig,
    source: &mut dyn FrameSource,
    opts: LiveOptions,
    truth: Option<&TruthCheck>,
) -> Result<LiveReport, String> {
    let keep_devtools = !source.is_live();
    let data = cfg.data_dir.clone();
    let research = cfg.research;
    let mut rt = Runtime::new(cfg).map_err(|e| format!("could not open the data folder {}: {e}", data.display()))?;
    let json = m.get_flag("json");
    let explain = m.get_flag("explain");
    eprintln!(
        "Syrup · data: {} · OCR: {} · research: {}",
        data.display(),
        rt.analyzer.ocr_engine(),
        if research { "on" } else { "off" }
    );
    let devtools = match m.get_one::<u16>("devtools") {
        Some(port) => {
            let d = Devtools::start(*port, rt.shared(), rt.bus.clone(), rt.commands())
                .map_err(|e| format!("could not start the devtools page on port {port}: {e}"))?;
            eprintln!("devtools: {}", d.url());
            Some(d)
        }
        None => None,
    };
    let events = rt.bus.subscribe();
    let mut next_state = 0u64;
    let mut scores = truth.map(|_| TruthScores::default());
    let opts = LiveOptions { print: !m.get_flag("quiet") && !json, ..opts };
    let report = run_with(&mut rt, source, &opts, |rt, frame, step| {
        if explain {
            for r in events.try_iter() {
                if let Some((ts, line)) = describe(&r.event) {
                    eprintln!("[{:>7.1}s] {line}", ts as f64 / 1000.0);
                }
            }
            if step.analysed && frame.timestamp_ms >= next_state {
                next_state = frame.timestamp_ms + 5000;
                eprintln!("[{:>7.1}s] sees: {}", frame.timestamp_ms as f64 / 1000.0, state_line(rt));
            }
        }
        if let (Some(t), Some(s)) = (truth, scores.as_mut())
            && step.analysed
        {
            t.score(rt, frame.timestamp_ms, s);
        }
    })
    .map_err(|e| e.to_string())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report.summary).unwrap_or_default());
    } else {
        print_summary(&rt, &report);
    }
    if let (Some(t), Some(s)) = (truth, scores) {
        t.report(&rt, &report, &s);
    }
    // A recording is over in seconds; keep the page up to look at what it left.
    if let Some(d) = devtools
        && keep_devtools
    {
        eprintln!("devtools stays up at {} — press Enter to quit", d.url());
        let _ = std::io::stdin().lock().read_line(&mut String::new());
    }
    Ok(report)
}

fn print_summary(rt: &Runtime, r: &LiveReport) {
    let s = &r.summary;
    let game = s.title.clone().unwrap_or_else(|| "(no game)".into());
    eprintln!();
    eprintln!(
        "Session {} · {} · {:.1} min · {} frames, {} analysed in {:.1} s",
        s.session,
        game,
        s.duration_s / 60.0,
        r.frames,
        r.analysed,
        r.wall_s
    );
    eprintln!(
        "{} deaths · {} wins · {} level-ups · Syrup spoke {} times, held back {}",
        s.deaths, s.victories, s.level_ups, s.advice_shown, s.advice_suppressed
    );
    if !s.concepts.is_empty() {
        eprintln!("learned: {}", s.concepts.join(", ").replace('_', " "));
    }
    if let Some(p) = rt.profile() {
        eprintln!(
            "profile: {} elements known · {} sessions · hat: {}",
            p.known_ui_elements.len(),
            p.stats.sessions,
            p.visual_identity.hat.name().replace('_', " ")
        );
    }
    if let Some(g) = rt.knowledge()
        && !g.facts.is_empty()
    {
        eprintln!("knowledge: {} facts from {} lookups", g.facts.len(), g.researched.len());
    }
    for line in &rt.view.summary {
        eprintln!("Syrup: {line}");
    }
    eprintln!("saved in {}", rt.memory.dir.session(&rt.session_id).display());
    if !r.overlay_pictures.is_empty() {
        eprintln!("{} overlay pictures", r.overlay_pictures.len());
    }
}

fn live(m: &ArgMatches) -> Result<(), String> {
    syrup_capture::init_process();
    let fps = *m.get_one::<f32>("fps").unwrap_or(&8.0);
    let mut source: Box<dyn FrameSource> = if m.get_flag("screen") {
        Box::new(syrup_capture::ScreenSource::new(fps).map_err(|e| e.to_string())?)
    } else {
        let selector = match (m.get_one::<String>("window"), m.get_one::<String>("exe")) {
            (Some(t), _) => WindowSelector::Title(t.clone()),
            (_, Some(e)) => WindowSelector::Executable(e.clone()),
            _ => WindowSelector::Auto,
        };
        eprintln!("watching {}", selector.describe());
        Box::new(syrup_capture::WindowSource::new(selector, fps).map_err(|e| e.to_string())?)
    };
    let cfg = config(m, true)?;
    let stop = Arc::new(AtomicBool::new(false));
    stop_on_enter(stop.clone());
    eprintln!("Press Enter to stop.");
    let opts = LiveOptions {
        seconds: m.get_one::<f64>("seconds").copied(),
        realtime: true,
        overlay: !m.get_flag("no-overlay"),
        voice: !m.get_flag("no-voice"),
        overlay_dir: m.get_one::<PathBuf>("overlay-dir").cloned(),
        print: true,
        linger: Duration::from_secs(8),
        stop: Some(stop),
    };
    go(m, cfg, &mut source, opts, None).map(|_| ())
}

fn replay(m: &ArgMatches) -> Result<(), String> {
    let path = m.get_one::<PathBuf>("path").unwrap();
    let fps = m.get_one::<f32>("fps").copied();
    let width = m.get_one::<u32>("width").copied();
    let start = m.get_one::<f64>("start").copied();
    let seconds = m.get_one::<f64>("seconds").copied();
    let mut source: Box<dyn FrameSource> = if path.is_file() && start.is_some() && !is_image(path) {
        // Decode only the part asked for.
        let mut v = syrup_capture::VideoSource::open_at(
            path,
            fps,
            width,
            start.unwrap_or(0.0) as f32,
            seconds.map(|s| s as f32),
        )
        .map_err(|e| e.to_string())?;
        if let Some(t) = m.get_one::<String>("title") {
            v = v.with_title(t);
        }
        Box::new(v)
    } else {
        if let Some(t) = m.get_one::<String>("title")
            && path.is_file()
            && !is_image(path)
        {
            Box::new(syrup_capture::VideoSource::open(path, fps, width).map_err(|e| e.to_string())?.with_title(t))
        } else {
            syrup_capture::open_recording(path, fps, width).map_err(|e| e.to_string())?
        }
    };
    let cfg = config(m, false)?;
    let realtime = m.get_flag("realtime");
    let stop = Arc::new(AtomicBool::new(false));
    if realtime {
        stop_on_enter(stop.clone());
    }
    let opts = LiveOptions {
        seconds,
        realtime,
        overlay: false,
        voice: false,
        overlay_dir: m.get_one::<PathBuf>("overlay-dir").cloned(),
        print: true,
        linger: Duration::ZERO,
        stop: Some(stop),
    };
    go(m, cfg, &mut source, opts, None).map(|_| ())
}

fn is_image(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("png" | "jpg" | "jpeg")
    )
}

fn simulate(m: &ArgMatches) -> Result<(), String> {
    let name = m.get_one::<String>("which").unwrap();
    let kind =
        GameKind::parse(name).ok_or_else(|| format!("no test game called \"{name}\" (dungeon, scroller, cards)"))?;
    let seed = *m.get_one::<u64>("seed").unwrap();
    let fps = *m.get_one::<f32>("fps").unwrap();
    let seconds = m.get_one::<f64>("seconds").copied().unwrap_or(120.0);
    let mut session = Session::of(kind, seed, fps, Some(seconds as f32));
    let check = m.get_flag("truth").then(|| TruthCheck { log: session.truth() });
    let cfg = config(m, false)?;
    let opts = LiveOptions {
        seconds: None,
        realtime: false,
        overlay: false,
        voice: false,
        overlay_dir: m.get_one::<PathBuf>("overlay-dir").cloned(),
        print: true,
        linger: Duration::ZERO,
        stop: None,
    };
    go(m, cfg, &mut session, opts, check.as_ref()).map(|_| ())
}

/// Syrup's view of a synthetic game against the game's own truth.
struct TruthCheck {
    log: Arc<std::sync::Mutex<syrup_testgames::TruthLog>>,
}

#[derive(Default)]
struct TruthScores {
    /// concept → (frames it was shown, frames Syrup had it, sum of |error|, values compared)
    concepts: BTreeMap<String, (u32, u32, f64, u32)>,
    /// (frames, frames whose scene Syrup got right)
    scenes: (u32, u32),
}

impl TruthCheck {
    fn score(&self, rt: &Runtime, t_ms: u64, s: &mut TruthScores) {
        let Ok(log) = self.log.lock() else { return };
        let Some(truth) = log.at(t_ms) else { return };
        s.scenes.0 += 1;
        if rt.last_observation().is_some_and(|o| o.scene.kind == truth.scene) {
            s.scenes.1 += 1;
        }
        for e in &truth.elements {
            let Some(want) = e.fraction() else { continue };
            let entry = s.concepts.entry(e.concept.clone()).or_default();
            entry.0 += 1;
            if let Some(c) = rt.state().concept(&e.concept) {
                entry.1 += 1;
                if let Some(got) = c.fraction() {
                    entry.2 += (got - want).abs();
                    entry.3 += 1;
                }
            }
        }
    }

    fn report(&self, rt: &Runtime, r: &LiveReport, s: &TruthScores) {
        let Ok(log) = self.log.lock() else { return };
        // Syrup counts every defeat screen (a death, a lost match) as a death.
        let deaths =
            log.events.iter().filter(|e| matches!(e.kind, TruthEventKind::Died | TruthEventKind::Defeat)).count();
        let wins = log.events.iter().filter(|e| matches!(e.kind, TruthEventKind::Victory)).count();
        let levels = log.events.iter().filter(|e| matches!(e.kind, TruthEventKind::LevelUp { .. })).count();
        eprintln!();
        eprintln!("against the truth:");
        eprintln!(
            "  deaths or defeats: {deaths} happened, {} seen · wins: {wins}, {} seen · level-ups: {levels}, {} seen",
            r.summary.deaths, r.summary.victories, r.summary.level_ups
        );
        if s.scenes.0 > 0 {
            eprintln!(
                "  scene right in {:.0}% of {} analysed frames",
                100.0 * s.scenes.1 as f64 / s.scenes.0 as f64,
                s.scenes.0
            );
        }
        for (name, (shown, had, err, n)) in &s.concepts {
            let mean = if *n > 0 { format!("{:.3}", err / *n as f64) } else { "–".into() };
            eprintln!(
                "  {name}: known in {:.0}% of the frames it was on screen · mean error {mean}",
                100.0 * *had as f64 / (*shown).max(1) as f64
            );
        }
        let _ = rt;
    }
}

fn research(m: &ArgMatches) -> Result<(), String> {
    let store = MemoryStore::open(data_dir(m)).map_err(|e| e.to_string())?;
    let title = m.get_one::<String>("title").unwrap();
    let (id, title) = match store.profiles().into_iter().find(|p| {
        p.game_id == *title || syrup_core::util::normalize_words(&p.title) == syrup_core::util::normalize_words(title)
    }) {
        Some(p) => (p.game_id, p.title),
        None => game_for_title(title),
    };
    let fetcher: Arc<dyn Fetcher> = match m.get_one::<PathBuf>("fixtures") {
        Some(dir) => Arc::new(Fixtures::new(dir)),
        None => Arc::new(Cached::new(Curl::default(), &store.dir.research_cache())),
    };
    let profile = store.load_profile(&id);
    let q = ResearchQuestion {
        game_id: id.clone(),
        game_title: title.clone(),
        topic: m.get_one::<String>("topic").cloned(),
        reason: "asked from the command line".into(),
        sources: profile.as_ref().map(|p| p.knowledge_sources.clone()).unwrap_or_else(|| {
            syrup_recognition::new_profile(
                &syrup_core::GameIdentity {
                    game_id: id.clone(),
                    title: title.clone(),
                    confidence: syrup_core::Confidence::new(1.0),
                    version: None,
                    platform: None,
                    evidence: vec![],
                    confirmed: true,
                },
                &Default::default(),
            )
            .knowledge_sources
        }),
        version: m
            .get_one::<String>("played")
            .cloned()
            .or_else(|| profile.as_ref().and_then(|p| p.current_version.clone())),
    };
    eprintln!("looking up {} …", q.describe());
    let out = ResearchAgent::new(fetcher).research(&q);
    for f in &out.failures {
        eprintln!("  ({f})");
    }
    let mut graph = store.load::<KnowledgeGraph>(&id, "knowledge.json").unwrap_or_else(|| KnowledgeGraph::new(&id));
    let added = syrup_knowledge::apply(&mut graph, &q, &out);
    store.save(&id, "knowledge.json", &graph).map_err(|e| e.to_string())?;
    println!("{} ({id}): {} facts, {added} new", title, out.facts.len());
    if !out.genres.is_empty() {
        println!("genres: {}", out.genres.join(", "));
    }
    if let Some(v) = &out.latest_version {
        println!("latest version: {v}");
    }
    for f in &out.facts {
        let flags = [(f.stale, "old version"), (f.spoiler != syrup_core::knowledge::SpoilerLevel::None, "spoiler")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, w)| *w)
            .collect::<Vec<_>>();
        println!(
            "- {} [{}{}]",
            f.claim,
            f.kind.word(),
            if flags.is_empty() { String::new() } else { format!(", {}", flags.join(", ")) }
        );
        println!(
            "    {} · {} · {}{}",
            f.source.title,
            f.source.url,
            f.confidence,
            f.game_version.as_ref().map(|v| format!(" · v{v}")).unwrap_or_default()
        );
    }
    for (a, rel, b, c) in &out.relations {
        println!("~ {a} {rel:?} {b} ({c})");
    }
    Ok(())
}

fn profiles(m: &ArgMatches) -> Result<(), String> {
    let store = MemoryStore::open(data_dir(m)).map_err(|e| e.to_string())?;
    let all = store.profiles();
    if let Some(which) = m.get_one::<String>("game") {
        let key = syrup_core::util::normalize_words(which);
        let p = all
            .iter()
            .find(|p| p.game_id == *which || syrup_core::util::normalize_words(&p.title) == key)
            .ok_or_else(|| format!("Syrup has not learned \"{which}\""))?;
        println!("{}", serde_json::to_string_pretty(p).unwrap_or_default());
        let facts = store.load::<KnowledgeGraph>(&p.game_id, "knowledge.json").map(|g| g.facts.len()).unwrap_or(0);
        eprintln!("{} facts known · {} episodes remembered", facts, store.episodes(&p.game_id).len());
        return Ok(());
    }
    if all.is_empty() {
        println!("Syrup has not learned any game yet (data: {}).", store.root().display());
        return Ok(());
    }
    for p in &all {
        let concepts: Vec<&str> = p.known_ui_elements.iter().filter_map(|e| e.concept.as_deref()).collect();
        println!(
            "{:<28} {:<22} {:>3} sessions {:>6.1} min  hat: {:<16} genres: {}  knows: {}",
            p.title,
            p.game_id,
            p.stats.sessions,
            p.stats.observed_ms as f64 / 60_000.0,
            p.visual_identity.hat.name(),
            if p.genres.is_empty() { "?".to_string() } else { p.genres.join(", ") },
            if concepts.is_empty() { "-".to_string() } else { concepts.join(", ") }
        );
    }
    Ok(())
}

fn forget(m: &ArgMatches) -> Result<(), String> {
    let store = MemoryStore::open(data_dir(m)).map_err(|e| e.to_string())?;
    let mut did = Vec::new();
    if m.get_flag("everything") {
        store.forget_everything().map_err(|e| e.to_string())?;
        println!("Forgot everything in {}.", store.root().display());
        return Ok(());
    }
    if let Some(which) = m.get_one::<String>("game") {
        let key = syrup_core::util::normalize_words(which);
        let id = store
            .profiles()
            .into_iter()
            .find(|p| p.game_id == *which || syrup_core::util::normalize_words(&p.title) == key)
            .map(|p| p.game_id)
            .unwrap_or_else(|| which.clone());
        if store.forget_game(&id).map_err(|e| e.to_string())? {
            did.push(format!("the game {id}"));
        }
    }
    if let Some(player) = m.get_one::<String>("player")
        && store.forget_player(player).map_err(|e| e.to_string())?
    {
        did.push(format!("the player model of {player}"));
    }
    if m.get_flag("recordings") && store.forget_recordings().map_err(|e| e.to_string())? {
        did.push("the recordings".into());
    }
    if m.get_flag("sessions") && store.forget_sessions().map_err(|e| e.to_string())? {
        did.push("the session timelines".into());
    }
    if did.is_empty() {
        println!("Nothing to forget.");
    } else {
        println!("Forgot {}.", did.join(", "));
    }
    Ok(())
}

fn avatar(m: &ArgMatches) -> Result<(), String> {
    let out = m.get_one::<PathBuf>("out").unwrap();
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let save = |name: &str, img: &image::RgbaImage| img.save(out.join(name)).map_err(|e| format!("{name}: {e}"));
    save("hats.png", &syrup_avatar::hat_gallery(150))?;
    for e in Expression::ALL {
        let pose =
            syrup_avatar::Pose { hat: Hat::SyrupCap, expression: e, speaking: false, t_ms: 0, reduced_motion: true };
        save(&format!("face-{}.png", e.name()), &syrup_avatar::render(&pose, 240))?;
    }
    let line = Line {
        advice_id: 1,
        topic: "health".into(),
        text: "Health is low. Back off and heal.".into(),
        source: "I saw it · 90%".into(),
    };
    let base = OverlayView {
        hat: Hat::KnightHelmet,
        expression: Expression::Warning,
        line: Some(line),
        status: "Watching Dungeon 3D.".into(),
        reduced_motion: true,
        ..Default::default()
    };
    for mode in [OverlayMode::Minimal, OverlayMode::Normal, OverlayMode::Analysis, OverlayMode::PostGame] {
        let mut v = base.clone();
        v.mode = mode;
        v.analysis = vec![
            "game: Dungeon 3D (0.98, learned)".into(),
            "scene: gameplay 0.84 · 6 regions · 4 texts".into(),
            "health: 23% (0.91)".into(),
            "ammo: 12 (0.88)".into(),
        ];
        if mode == OverlayMode::PostGame {
            v.line = None;
            v.expression = Expression::Proud;
            v.summary = vec![
                "Twelve minutes of Dungeon 3D.".into(),
                "Two deaths, both to ambushes.".into(),
                "Learned: health, ammo, gold.".into(),
            ];
        }
        let name = format!("overlay-{}.png", format!("{mode:?}").to_lowercase());
        save(&name, &syrup_ui::paint(&v, 0).image)?;
    }
    println!("Drew Syrup in {}.", out.display());
    Ok(())
}

fn windows() -> Result<(), String> {
    syrup_capture::init_process();
    let all = syrup_capture::list_windows();
    if all.is_empty() {
        return Err("no windows to list (live capture needs Windows)".into());
    }
    let pid = std::process::id();
    let picked = syrup_capture::pick_window(&all, &WindowSelector::Auto, pid);
    for w in &all {
        let mark = if picked.as_ref().is_some_and(|p| p.handle == w.handle) {
            "→"
        } else if syrup_capture::is_excluded(w, pid) {
            " "
        } else {
            "·"
        };
        println!(
            "{mark} {:<40} {:<24} {}x{}{}",
            w.title,
            w.executable.clone().unwrap_or_default(),
            w.client.w,
            w.client.h,
            if w.minimized { " (minimised)" } else { "" }
        );
    }
    println!("→ is the window `syrup live` would watch now; · could be picked with --window or --exe.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_is_consistent() {
        cli().debug_assert();
        let m = cli()
            .try_get_matches_from([
                "syrup",
                "simulate",
                "cards",
                "--seconds",
                "5",
                "--devtools",
                "--game",
                "High Card Duel",
            ])
            .unwrap();
        let (_, sub) = m.subcommand().unwrap();
        assert_eq!(sub.get_one::<u16>("devtools"), Some(&7777));
        let cfg = config(sub, false).unwrap();
        assert!(!cfg.research);
        assert_eq!(cfg.confirm.as_ref().map(|c| c.0.as_str()), Some("high-card-duel"));
        let m = cli()
            .try_get_matches_from([
                "syrup",
                "live",
                "--exe",
                "MapleStory.exe",
                "--spoilers",
                "hints_only",
                "--mode",
                "minimal",
            ])
            .unwrap();
        let (_, sub) = m.subcommand().unwrap();
        let cfg = config(sub, true).unwrap();
        assert!(cfg.research);
        assert_eq!(cfg.overlay_mode, OverlayMode::Minimal);
        assert_eq!(cfg.coach.spoilers, SpoilerPolicy::HintsOnly);
        assert!(cli().try_get_matches_from(["syrup", "live", "--window", "a", "--exe", "b"]).is_err());
    }
}
