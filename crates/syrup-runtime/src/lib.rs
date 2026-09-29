//! Syrup Universal, wired together.
//!
//! [`Runtime::on_frame`] is the whole pipeline for one captured frame:
//! sample → perceive → (plugin) → recognise the game → track state → learn
//! the profile → update the player model → take in finished research →
//! coach → overlay. Everything that happens is published on the
//! [`EventBus`], written to the session's timeline, and mirrored into a
//! [`Snapshot`] the devtools page reads. Research runs on its own thread;
//! OCR can too; nothing on this path waits for the network.

pub mod devtools;
pub mod live;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use syrup_coach::{Announcement, CoachConfig, CoachContext, CoachEngine, FeedbackEffect};
use syrup_core::knowledge::{Fact, FactKind};
use syrup_core::observation::ObservedEvent;
use syrup_core::util::{normalize_words, now_iso, unix_now};
use syrup_core::{
    Advice, Event, EventBus, Expression, Feedback, FeedbackKind, Frame, GameIdentity, GameProfile, GameState, Hat,
    HudMark, IdentityCues, NormRect, Observation, TransitionKind, UiKind,
};
use syrup_knowledge::{Cached, Curl, Fetcher, KnowledgeGraph, ResearchQuestion, ResearchRecord, ResearchWorker};
use syrup_memory::{Episode, MemoryStore, ProfileLearner, SessionLog, SessionSummary};
use syrup_perception::{FrameSampler, OcrEngine, PerceptionConfig, SamplerConfig, SceneAnalyzer};
use syrup_player::{GameModel, PlayerModel, PlayerTracker};
use syrup_plugins::{PluginContext, PluginManager};
use syrup_recognition::{Recognizer, new_profile};
use syrup_state::StateEngine;
use syrup_ui::{Line, OverlayMode, OverlayView, UiAction};

pub struct RuntimeConfig {
    pub data_dir: PathBuf,
    pub player_id: String,
    /// Look things up on the web (only search terms leave the computer).
    pub research: bool,
    /// Where research fetches from (default: `curl`, cached in the data folder).
    pub fetcher: Option<Arc<dyn Fetcher>>,
    pub ocr: Arc<dyn OcrEngine>,
    pub perception: PerceptionConfig,
    pub sampler: SamplerConfig,
    pub coach: CoachConfig,
    pub overlay_mode: OverlayMode,
    pub reduced_motion: bool,
    /// Keep a frame a second in `recordings/` (off unless asked).
    pub record: bool,
    pub save_every_ms: u64,
    /// Extra data plugins (`*/plugin.json`).
    pub plugins_dir: Option<PathBuf>,
    /// The player says which game this is.
    pub confirm: Option<(String, String)>,
    /// Write the session's timeline.
    pub timeline: bool,
}

impl RuntimeConfig {
    pub fn new(data_dir: impl Into<PathBuf>, ocr: Arc<dyn OcrEngine>) -> Self {
        RuntimeConfig {
            data_dir: data_dir.into(),
            player_id: "default".into(),
            research: false,
            fetcher: None,
            ocr,
            perception: PerceptionConfig::default(),
            sampler: SamplerConfig::default(),
            coach: CoachConfig::default(),
            overlay_mode: OverlayMode::Normal,
            reduced_motion: false,
            record: false,
            save_every_ms: 30_000,
            plugins_dir: None,
            confirm: None,
            timeline: true,
        }
    }
}

/// Things other threads ask the runtime to do (the devtools page, mostly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Feedback {
        advice_id: u64,
        topic: String,
        kind: FeedbackKind,
    },
    /// "This game is …".
    Confirm {
        game_id: String,
        title: String,
    },
    /// "That element is …" (an empty concept: "it is nothing").
    Correct {
        norm: NormRect,
        kind: String,
        concept: String,
    },
    Mode {
        mode: OverlayMode,
    },
    Research {
        topic: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceRecord {
    pub advice: Advice,
    pub shown: bool,
    pub reason: Option<String>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SceneSummary {
    pub kind: String,
    pub confidence: f32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub game_id: String,
    pub title: String,
    pub genres: Vec<String>,
    pub hat: String,
    pub sessions: u64,
    pub observed_min: f64,
    pub elements: Vec<syrup_core::KnownUiElement>,
    pub terms: Vec<(String, u64)>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Fps {
    pub captured: f32,
    pub analysed: f32,
}

/// Everything the devtools page shows.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub session: String,
    pub t_ms: u64,
    pub game: Option<GameIdentity>,
    pub candidates: Vec<GameIdentity>,
    pub plugin: Option<String>,
    pub ocr: String,
    pub research: bool,
    pub scene: Option<SceneSummary>,
    pub state: GameState,
    pub profile: Option<ProfileSummary>,
    pub player: Option<GameModel>,
    pub facts: Vec<Fact>,
    pub researched: Vec<ResearchRecord>,
    pub advice: Vec<AdviceRecord>,
    pub timings: BTreeMap<String, f32>,
    pub fps: Fps,
    pub regions: usize,
    pub texts: Vec<String>,
    pub uncertainties: Vec<String>,
    pub view: Option<OverlayView>,
}

/// What the devtools server reads.
#[derive(Default)]
pub struct Shared {
    pub snapshot: Snapshot,
    pub frame: Option<Arc<RgbaImage>>,
    pub observation: Option<Observation>,
}

/// What one frame produced.
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub analysed: bool,
    /// Lines to say now.
    pub shown: Vec<Advice>,
}

#[derive(Debug, Default, Clone)]
struct Counters {
    deaths: u32,
    victories: u32,
    level_ups: u32,
    shown: u32,
    suppressed: u32,
    feedback: BTreeMap<String, u32>,
    learned: Vec<String>,
}

pub struct Runtime {
    pub cfg: RuntimeConfig,
    pub bus: EventBus,
    pub memory: MemoryStore,
    pub session_id: String,
    started_at: String,
    log: Option<SessionLog>,
    log_seq: u64,
    sampler: FrameSampler,
    pub analyzer: SceneAnalyzer,
    recognizer: Recognizer,
    profiles: Vec<GameProfile>,
    identity: Option<GameIdentity>,
    candidates: Vec<GameIdentity>,
    announced: bool,
    last_uncertain_ms: u64,
    profile: Option<GameProfile>,
    learner: ProfileLearner,
    state: StateEngine,
    plugins: PluginManager,
    knowledge: Option<KnowledgeGraph>,
    research: Option<ResearchWorker>,
    researching: BTreeSet<String>,
    player: PlayerModel,
    tracker: PlayerTracker,
    coach: CoachEngine,
    announcements: Vec<Announcement>,
    pub view: OverlayView,
    line_until: u64,
    last_obs: Option<Observation>,
    source_key: Option<(Option<String>, Option<String>)>,
    last_recognition_ms: Option<u64>,
    screen_text: VecDeque<String>,
    last_save_ms: u64,
    last_profile_event_ms: u64,
    heartbeat: (u64, u32, u32),
    fps: Fps,
    timings: BTreeMap<String, f32>,
    advice_log: VecDeque<AdviceRecord>,
    /// When each topic was last logged as proposed, and as held back.
    proposed: BTreeMap<String, u64>,
    held_back: BTreeMap<String, u64>,
    commands: (Sender<Command>, Receiver<Command>),
    shared: Arc<Mutex<Shared>>,
    counters: Counters,
    first_ms: Option<u64>,
    last_ms: u64,
    last_recorded_ms: Option<u64>,
    finished: bool,
}

fn place(n: &NormRect) -> &'static str {
    n.place()
}

impl Runtime {
    pub fn new(cfg: RuntimeConfig) -> io::Result<Self> {
        let memory = MemoryStore::open(&cfg.data_dir)?;
        let session_id = memory.new_session_id(unix_now());
        let log = if cfg.timeline { SessionLog::create(&memory, &session_id).ok() } else { None };
        let profiles = memory.profiles();
        let player =
            memory.load_player::<PlayerModel>(&cfg.player_id).unwrap_or_else(|| PlayerModel::new(&cfg.player_id));
        let research = if cfg.research {
            let fetcher: Arc<dyn Fetcher> = cfg
                .fetcher
                .clone()
                .unwrap_or_else(|| Arc::new(Cached::new(Curl::default(), &memory.dir.research_cache())));
            Some(ResearchWorker::start(fetcher))
        } else {
            None
        };
        let mut plugins = PluginManager::with_builtins();
        if let Some(dir) = &cfg.plugins_dir {
            plugins.load_dir(dir);
        }
        let mut recognizer = Recognizer::new();
        if let Some((id, title)) = &cfg.confirm {
            recognizer.confirm(id, title);
        }
        let analyzer = SceneAnalyzer::new(cfg.perception.clone(), cfg.ocr.clone());
        let view = OverlayView {
            mode: cfg.overlay_mode,
            reduced_motion: cfg.reduced_motion,
            status: "Looking at the screen.".into(),
            ..Default::default()
        };
        let coach = CoachEngine::new(cfg.coach.clone());
        Ok(Runtime {
            sampler: FrameSampler::new(cfg.sampler.clone()),
            cfg,
            bus: EventBus::default(),
            memory,
            session_id,
            started_at: now_iso(),
            log,
            log_seq: 0,
            analyzer,
            recognizer,
            profiles,
            identity: None,
            candidates: Vec::new(),
            announced: false,
            last_uncertain_ms: 0,
            profile: None,
            learner: ProfileLearner::new(),
            state: StateEngine::new(),
            plugins,
            knowledge: None,
            research,
            researching: BTreeSet::new(),
            player,
            tracker: PlayerTracker::new(),
            coach,
            announcements: Vec::new(),
            view,
            line_until: 0,
            last_obs: None,
            source_key: None,
            last_recognition_ms: None,
            screen_text: VecDeque::new(),
            last_save_ms: 0,
            last_profile_event_ms: 0,
            heartbeat: (0, 0, 0),
            fps: Fps::default(),
            timings: BTreeMap::new(),
            advice_log: VecDeque::new(),
            proposed: BTreeMap::new(),
            held_back: BTreeMap::new(),
            commands: channel(),
            shared: Arc::new(Mutex::new(Shared::default())),
            counters: Counters::default(),
            first_ms: None,
            last_ms: 0,
            last_recorded_ms: None,
            finished: false,
        })
    }

    pub fn commands(&self) -> Sender<Command> {
        self.commands.0.clone()
    }

    pub fn shared(&self) -> Arc<Mutex<Shared>> {
        self.shared.clone()
    }

    pub fn identity(&self) -> Option<&GameIdentity> {
        self.identity.as_ref()
    }

    pub fn profile(&self) -> Option<&GameProfile> {
        self.profile.as_ref()
    }

    pub fn state(&self) -> &GameState {
        self.state.state()
    }

    pub fn knowledge(&self) -> Option<&KnowledgeGraph> {
        self.knowledge.as_ref()
    }

    pub fn player(&self) -> &PlayerModel {
        &self.player
    }

    pub fn last_observation(&self) -> Option<&Observation> {
        self.last_obs.as_ref()
    }

    pub fn advice_log(&self) -> impl Iterator<Item = &AdviceRecord> {
        self.advice_log.iter()
    }

    pub fn research_pending(&self) -> usize {
        self.research.as_ref().map(|r| r.pending()).unwrap_or(0)
    }

    fn game_id(&self) -> String {
        self.identity.as_ref().map(|i| i.game_id.clone()).unwrap_or_else(|| "unknown".into())
    }

    fn publish(&self, e: Event) {
        self.bus.publish(e);
    }

    /// Writes new bus events to the session timeline.
    fn flush_log(&mut self) {
        let records = self.bus.since(self.log_seq);
        if let Some(last) = records.last() {
            self.log_seq = last.seq;
        }
        if let Some(log) = self.log.as_mut() {
            for r in &records {
                if !matches!(r.event, Event::FrameCaptured { .. }) {
                    log.write(r);
                }
            }
            log.flush();
        }
    }

    /// One captured frame through the whole pipeline.
    pub fn on_frame(&mut self, frame: &Frame) -> Step {
        let now = frame.timestamp_ms;
        self.first_ms.get_or_insert(now);
        self.last_ms = now;
        self.heartbeat.1 += 1;
        if now.saturating_sub(self.heartbeat.0) >= 1000 {
            let secs = (now.saturating_sub(self.heartbeat.0)).max(1) as f32 / 1000.0;
            if self.heartbeat.0 != 0 {
                self.fps = Fps { captured: self.heartbeat.1 as f32 / secs, analysed: self.heartbeat.2 as f32 / secs };
            }
            self.publish(Event::FrameCaptured {
                ts_ms: now,
                frame: frame.index,
                width: frame.width(),
                height: frame.height(),
                captured_per_s: self.fps.captured,
                analysed_per_s: self.fps.analysed,
            });
            self.heartbeat = (now, 0, 0);
        }
        let commands: Vec<Command> = self.commands.1.try_iter().collect();
        for c in commands {
            self.on_command(c, now);
        }
        let decision = self.sampler.decide(frame);
        if !decision.analyse {
            return Step::default();
        }
        self.heartbeat.2 += 1;
        let started = Instant::now();
        // A different window: start over on what the screen is.
        let key = (frame.source.window_title.clone(), frame.source.executable.clone());
        if self.source_key.as_ref() != Some(&key) {
            if self.source_key.is_some() {
                self.recognizer.reset();
                if let Some((id, title)) = &self.cfg.confirm {
                    self.recognizer.confirm(id, title);
                }
                self.analyzer.reset();
                self.last_recognition_ms = None;
                self.publish(Event::Note {
                    ts_ms: now,
                    message: format!("now watching {}", key.0.clone().unwrap_or_else(|| "another window".into())),
                });
            }
            self.source_key = Some(key);
        }

        let t = Instant::now();
        let mut obs = self.analyzer.analyze(frame);
        self.timings.insert("perception".into(), t.elapsed().as_secs_f32() * 1000.0);
        if let Some(p) = self.plugins.active() {
            p.parse_observation(frame, &mut obs);
        }
        for e in &obs.events {
            match e {
                ObservedEvent::RegionAppeared { region } => {
                    if let Some(r) = obs.region(*region) {
                        self.publish(Event::UiElementDiscovered {
                            ts_ms: now,
                            region: *region,
                            kind: r.kind.word().to_string(),
                            place: r.norm.place().to_string(),
                        });
                    }
                }
                ObservedEvent::SceneChanged { from, to } => {
                    self.publish(Event::SceneChanged { ts_ms: now, from: *from, to: *to })
                }
                _ => {}
            }
        }
        for t in obs.text.iter().filter(|t| t.fresh) {
            let s = t.text.trim().to_string();
            if s.len() >= 3 && !self.screen_text.contains(&s) {
                self.screen_text.push_back(s);
                while self.screen_text.len() > 60 {
                    self.screen_text.pop_front();
                }
            }
        }

        let t = Instant::now();
        self.maybe_recognize(frame, &obs, now);
        self.timings.insert("recognition".into(), t.elapsed().as_secs_f32() * 1000.0);

        let t = Instant::now();
        let up = self.state.update(&obs);
        if let Some(p) = self.plugins.active() {
            p.extract_state(&obs, self.state.state_mut());
        }
        self.timings.insert("state".into(), t.elapsed().as_secs_f32() * 1000.0);
        let game_id = self.game_id();
        for tr in &up.transitions {
            self.publish(Event::StateChanged { transition: tr.clone() });
            let episode = match tr.kind {
                TransitionKind::PlayerDied => {
                    self.counters.deaths += 1;
                    self.publish(Event::PlayerDied { ts_ms: now, context: tr.detail.clone() });
                    Some((
                        "death",
                        if tr.detail.is_empty() { "died".to_string() } else { format!("died {}", tr.detail) },
                    ))
                }
                TransitionKind::Victory => {
                    self.counters.victories += 1;
                    Some(("victory", "won".to_string()))
                }
                TransitionKind::LevelUp => {
                    self.counters.level_ups += 1;
                    Some(("level_up", "levelled up".to_string()))
                }
                TransitionKind::RepeatedFailure => Some(("repeated_failure", tr.detail.clone())),
                TransitionKind::ObjectiveChanged => {
                    self.publish(Event::ObjectiveChanged { ts_ms: now, text: tr.detail.clone() });
                    None
                }
                _ => None,
            };
            if let (Some((kind, summary)), true) = (episode, self.identity.is_some()) {
                let _ = self.memory.add_episode(
                    &game_id,
                    Episode {
                        at: now_iso(),
                        session: self.session_id.clone(),
                        kind: kind.into(),
                        summary,
                        details: BTreeMap::new(),
                    },
                );
            }
        }
        for l in &up.learned {
            self.publish(Event::ConceptLearned {
                ts_ms: now,
                concept: l.concept.clone(),
                source: l.source.clone(),
                confidence: l.confidence,
                evidence: l.evidence.clone(),
            });
            if !self.counters.learned.contains(&l.concept) {
                self.counters.learned.push(l.concept.clone());
            }
            self.announcements.push(Announcement::Learned {
                concept: l.concept.clone(),
                place: place(&l.norm).to_string(),
                kind: l.kind.clone(),
            });
        }
        if let Some(profile) = self.profile.as_mut() {
            self.learner.observe(profile, &obs);
            self.learner.learn_concepts(profile, &up.learned);
        }
        let changes = self.learner.take_changes();
        if !changes.is_empty() && now.saturating_sub(self.last_profile_event_ms) >= 3000 {
            self.last_profile_event_ms = now;
            self.publish(Event::ProfileUpdated { ts_ms: now, game_id: game_id.clone(), reason: changes.join("; ") });
        }
        let insights = self.tracker.update(&mut self.player, &game_id, self.state.state(), &up.transitions);
        for i in insights {
            self.announcements.push(Announcement::Insight(i));
        }
        self.poll_research(now);

        // Coaching.
        let t = Instant::now();
        let extra = match (self.plugins.active(), self.identity.as_ref()) {
            (Some(p), Some(identity)) => p.advice(&PluginContext {
                now_ms: now,
                identity,
                observation: &obs,
                state: self.state.state(),
                profile: self.profile.as_ref(),
            }),
            _ => Vec::new(),
        };
        let announcements = std::mem::take(&mut self.announcements);
        let ctx = CoachContext {
            now_ms: now,
            identity: self.identity.as_ref(),
            state: self.state.state(),
            transitions: &up.transitions,
            observation: Some(&obs),
            knowledge: self.knowledge.as_ref(),
            player: self.player.games.get(&game_id),
            announcements: &announcements,
            extra,
        };
        let out = self.coach.step(&ctx);
        self.timings.insert("coach".into(), t.elapsed().as_secs_f32() * 1000.0);
        // While a situation lasts (health stays low) the same advice is proposed
        // on every analysis: the timeline gets it once every few seconds.
        let shown_ids: BTreeSet<u64> = out.shown.iter().map(|a| a.id).collect();
        for a in &out.candidates {
            if shown_ids.contains(&a.id) || once_in_a_while(&mut self.proposed, &a.topic, now) {
                self.publish(Event::AdviceGenerated { advice: a.clone() });
            }
        }
        for a in &out.shown {
            self.publish(Event::AdviceShown { ts_ms: now, advice_id: a.id, text: a.text.clone() });
            self.record_advice(a.clone(), true, None, now);
        }
        for (a, why) in &out.suppressed {
            if once_in_a_while(&mut self.held_back, &a.topic, now) {
                self.publish(Event::AdviceSuppressed {
                    ts_ms: now,
                    advice_id: a.id,
                    topic: a.topic.clone(),
                    reason: why.clone(),
                });
                self.record_advice(a.clone(), false, Some(why.clone()), now);
                self.counters.suppressed += 1;
            }
        }
        self.counters.shown += out.shown.len() as u32;
        for (topic, why) in out.research {
            self.ask(Some(topic), &why, now);
        }
        self.update_view(now, &out.shown);
        self.timings.insert("total".into(), started.elapsed().as_secs_f32() * 1000.0);
        self.sampler.record_cost(started.elapsed().as_secs_f32() * 1000.0);

        if self.cfg.record && self.last_recorded_ms.is_none_or(|t| now.saturating_sub(t) >= 1000) {
            self.last_recorded_ms = Some(now);
            let dir = self.memory.recording_dir(&self.session_id);
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = frame.image.save(dir.join(format!("{now}ms.png")));
            }
        }
        if now.saturating_sub(self.last_save_ms) >= self.cfg.save_every_ms {
            self.last_save_ms = now;
            self.save_all();
        }
        self.flush_log();
        self.publish_snapshot(frame, &obs);
        self.last_obs = Some(obs);
        Step { analysed: true, shown: out.shown }
    }

    fn record_advice(&mut self, advice: Advice, shown: bool, reason: Option<String>, at_ms: u64) {
        self.advice_log.push_back(AdviceRecord { advice, shown, reason, at_ms });
        while self.advice_log.len() > 200 {
            self.advice_log.pop_front();
        }
    }

    fn cues(&self, frame: &Frame, obs: &Observation) -> IdentityCues {
        IdentityCues {
            window_title: frame.source.window_title.clone(),
            executable: frame.source.executable.clone(),
            executable_path: frame.source.executable_path.clone(),
            screen_text: self.screen_text.iter().cloned().collect(),
            hud: obs
                .ui_regions
                .iter()
                .filter(|r| r.confidence.at_least(0.5))
                .map(|r| HudMark { norm: r.norm, appearance: r.appearance, kind: syrup_kind(&r.kind).into() })
                .collect(),
        }
    }

    fn maybe_recognize(&mut self, frame: &Frame, obs: &Observation, now: u64) {
        let confident = self.identity.as_ref().is_some_and(|i| i.is_confident());
        let every = if self.identity.is_none() {
            0
        } else if confident {
            5000
        } else {
            1000
        };
        if self.last_recognition_ms.is_some_and(|t| now.saturating_sub(t) < every) {
            return;
        }
        self.last_recognition_ms = Some(now);
        let cues = self.cues(frame, obs);
        let r = self.recognizer.recognize(&cues, &self.profiles);
        self.candidates = r.candidates.clone();
        let best = r.best;
        match self.identity.clone() {
            None => self.switch_game(best, &cues, now),
            Some(cur) if cur.game_id != best.game_id => {
                let better =
                    best.is_confident() || (!cur.is_known() && best.is_known() && best.confidence.at_least(0.5));
                if better {
                    self.switch_game(best, &cues, now);
                }
            }
            Some(_) => {
                let became_sure = best.is_confident() && !self.announced;
                if let Some(p) = self.profile.as_mut()
                    && let Some(v) = &best.version
                {
                    p.current_version = Some(v.clone());
                }
                self.identity = Some(best.clone());
                if became_sure {
                    self.announced = true;
                    self.publish(Event::GameIdentified { ts_ms: now, identity: best.clone() });
                    let first_time = self.profile.as_ref().is_none_or(|p| p.stats.sessions <= 1);
                    self.announcements.push(Announcement::Game { identity: best, first_time });
                } else if !best.is_confident() && now.saturating_sub(self.last_uncertain_ms) >= 15_000 {
                    self.last_uncertain_ms = now;
                    self.publish(Event::GameUncertain { ts_ms: now, best: Some(best) });
                }
            }
        }
    }

    fn switch_game(&mut self, identity: GameIdentity, cues: &IdentityCues, now: u64) {
        self.save_all();
        let id = identity.game_id.clone();
        let mut profile = self.memory.load_profile(&id).unwrap_or_else(|| new_profile(&identity, cues));
        if let Some(exe) = &cues.executable
            && !profile.executables.iter().any(|x| x.eq_ignore_ascii_case(exe))
        {
            profile.executables.push(exe.clone());
        }
        if let Some(t) = &cues.window_title
            && !t.trim().is_empty()
            && !profile.window_titles.iter().any(|x| normalize_words(x) == normalize_words(t))
        {
            profile.window_titles.push(t.clone());
        }
        if identity.version.is_some() {
            profile.current_version = identity.version.clone();
        }
        let plugin = self.plugins.activate_for(cues, &identity);
        let mut graph =
            self.memory.load::<KnowledgeGraph>(&id, "knowledge.json").unwrap_or_else(|| KnowledgeGraph::new(&id));
        let mut hints = syrup_memory::learner::hints(&profile);
        let mut scene_words = Vec::new();
        if let Some(p) = self.plugins.active() {
            if let Some(theme) = p.visual_theme() {
                profile.visual_identity = theme;
            }
            for g in p.genres() {
                if !profile.genres.contains(&g) {
                    profile.genres.push(g);
                }
            }
            for s in p.knowledge_sources() {
                if !profile.knowledge_sources.contains(&s) {
                    profile.knowledge_sources.push(s);
                }
            }
            for f in p.seed_facts() {
                graph.add_fact(f);
            }
            for n in p.known_entities() {
                graph.nodes.entry(n.id.clone()).or_insert(n);
            }
            hints.extend(p.known_regions());
            scene_words = p.scene_words();
        } else if profile.visual_identity.hat == Hat::SyrupCap && !profile.genres.is_empty() {
            profile.visual_identity.hat = Hat::for_genres(&profile.genres);
        }
        if let Some(v) = &profile.current_version {
            graph.mark_stale(v);
        }
        self.state.reset();
        self.state.set_hints(hints);
        self.analyzer.cfg.scene_words = scene_words;
        self.learner.start_session(&mut profile);
        let first_time = profile.stats.sessions <= 1;
        self.announced = identity.is_confident();
        if self.announced {
            self.publish(Event::GameIdentified { ts_ms: now, identity: identity.clone() });
        } else {
            self.last_uncertain_ms = now;
            self.publish(Event::GameUncertain { ts_ms: now, best: Some(identity.clone()) });
        }
        if let Some(pid) = &plugin {
            self.publish(Event::PluginActivated { ts_ms: now, plugin: pid.clone(), game_id: id.clone() });
        }
        self.publish(Event::ProfileUpdated {
            ts_ms: now,
            game_id: id.clone(),
            reason: if first_time {
                "a new profile".into()
            } else {
                format!("loaded: session {}", profile.stats.sessions)
            },
        });
        self.announcements.push(Announcement::Game { identity: identity.clone(), first_time });
        let overview_known = graph.researched.iter().any(|r| r.question == identity.title && r.ok);
        self.profiles.retain(|p| p.game_id != id);
        self.profiles.push(profile.clone());
        self.profile = Some(profile);
        self.knowledge = Some(graph);
        self.identity = Some(identity);
        if !overview_known {
            self.ask(None, "a game Syrup has not looked up yet", now);
        }
    }

    /// Asks the research worker, once per topic.
    fn ask(&mut self, topic: Option<String>, reason: &str, now: u64) {
        let (Some(identity), Some(worker)) = (self.identity.as_ref(), self.research.as_mut()) else { return };
        let key = format!("{}|{}", identity.game_id, topic.clone().unwrap_or_default());
        if !self.researching.insert(key) {
            return;
        }
        let q = ResearchQuestion {
            game_id: identity.game_id.clone(),
            game_title: identity.title.clone(),
            topic: topic.clone(),
            reason: reason.into(),
            sources: self.profile.as_ref().map(|p| p.knowledge_sources.clone()).unwrap_or_default(),
            version: self.profile.as_ref().and_then(|p| p.current_version.clone()),
        };
        self.bus.publish(Event::ResearchRequested {
            ts_ms: now,
            game_id: q.game_id.clone(),
            question: q.describe(),
            reason: reason.into(),
        });
        worker.ask(q);
    }

    fn poll_research(&mut self, now: u64) {
        let Some(worker) = self.research.as_mut() else { return };
        for (q, outcome) in worker.poll() {
            let same_game = self.identity.as_ref().is_some_and(|i| i.game_id == q.game_id);
            let mut graph = if same_game {
                self.knowledge.take().unwrap_or_else(|| KnowledgeGraph::new(&q.game_id))
            } else {
                self.memory.load(&q.game_id, "knowledge.json").unwrap_or_else(|| KnowledgeGraph::new(&q.game_id))
            };
            let added = syrup_knowledge::apply(&mut graph, &q, &outcome);
            let _ = self.memory.save(&q.game_id, "knowledge.json", &graph);
            if same_game {
                self.knowledge = Some(graph);
                if let Some(p) = self.profile.as_mut() {
                    let mut changed = Vec::new();
                    for g in &outcome.genres {
                        if !p.genres.contains(g) {
                            p.genres.push(g.clone());
                            changed.push(format!("genre {g}"));
                        }
                    }
                    for s in &outcome.sources_found {
                        if !p.knowledge_sources.contains(s) {
                            p.knowledge_sources.push(s.clone());
                        }
                    }
                    if p.current_version.is_none() && outcome.latest_version.is_some() {
                        p.current_version = outcome.latest_version.clone();
                        changed.push("version".into());
                    }
                    // A new game's hat follows its genres (plugins choose their own).
                    if self.plugins.active_id().is_none()
                        && p.visual_identity.hat == Hat::SyrupCap
                        && !p.genres.is_empty()
                    {
                        p.visual_identity.hat = Hat::for_genres(&p.genres);
                        changed.push(format!("hat: {}", p.visual_identity.hat.name()));
                    }
                    if !changed.is_empty() {
                        self.bus.publish(Event::ProfileUpdated {
                            ts_ms: now,
                            game_id: q.game_id.clone(),
                            reason: changed.join(", "),
                        });
                    }
                }
            }
            if outcome.facts.is_empty() {
                self.bus.publish(Event::ResearchFailed {
                    ts_ms: now,
                    game_id: q.game_id.clone(),
                    question: q.describe(),
                    reason: outcome.failures.join("; "),
                });
            } else {
                self.bus.publish(Event::KnowledgeUpdated {
                    ts_ms: now,
                    game_id: q.game_id.clone(),
                    question: q.describe(),
                    facts: added,
                    sources: outcome.sources_used.len(),
                });
                if same_game {
                    self.announcements.push(Announcement::Researched {
                        topic: q.topic.clone().unwrap_or_else(|| q.game_title.clone()),
                        facts: added,
                    });
                }
            }
        }
    }

    fn on_command(&mut self, c: Command, now: u64) {
        match c {
            Command::Feedback { advice_id, topic, kind } => {
                self.on_action(UiAction::Feedback { advice_id, topic, kind })
            }
            Command::Confirm { game_id, title } => {
                self.recognizer.confirm(&game_id, &title);
                self.cfg.confirm = Some((game_id.clone(), title.clone()));
                if let Some(p) = self.profile.as_mut()
                    && p.game_id == game_id
                {
                    p.title = title;
                }
                self.last_recognition_ms = None;
            }
            Command::Correct { norm, kind, concept } => {
                if let Some(p) = self.profile.as_mut() {
                    self.learner.correct(p, norm, &kind, &concept);
                    self.state.set_hints(syrup_memory::learner::hints(p));
                    let _ = self.memory.save_profile(p);
                }
                self.publish(Event::ProfileUpdated {
                    ts_ms: now,
                    game_id: self.game_id(),
                    reason: format!("the player says: {concept}"),
                });
            }
            Command::Mode { mode } => self.view.mode = mode,
            Command::Research { topic } => self.ask(Some(topic), "the player asked", now),
        }
    }

    /// The player clicked something on the overlay.
    pub fn on_action(&mut self, action: UiAction) {
        let now = self.last_ms;
        match action {
            UiAction::NextMode => self.view.mode = self.view.mode.next(),
            UiAction::Feedback { advice_id, topic, kind } => {
                let fb = Feedback { advice_id, topic: topic.clone(), kind, at_ms: now };
                *self.counters.feedback.entry(format!("{kind:?}").to_lowercase()).or_insert(0) += 1;
                self.publish(Event::FeedbackReceived { feedback: fb.clone() });
                let game_id = self.game_id();
                let effect = self.coach.feedback(&fb, self.player.game(&game_id));
                match effect {
                    FeedbackEffect::Explain(a) => {
                        self.publish(Event::AdviceShown { ts_ms: now, advice_id: a.id, text: a.text.clone() });
                        self.record_advice(a.clone(), true, None, now);
                        self.update_view(now, &[a]);
                    }
                    FeedbackEffect::Research(t) => self.ask(Some(t), "the player asked", now),
                    FeedbackEffect::DoubtConcept(concept) => {
                        self.state.doubt(&concept);
                        if let Some(p) = self.profile.as_mut()
                            && let Some(e) = p
                                .known_ui_elements
                                .iter_mut()
                                .find(|e| e.concept.as_deref() == Some(concept.as_str()) && !e.corrected)
                        {
                            e.confidence = syrup_core::Confidence::new(e.confidence.value() * 0.5);
                        }
                    }
                    FeedbackEffect::Nothing => {}
                }
                if matches!(kind, FeedbackKind::Ignore | FeedbackKind::StopSuggesting)
                    && self.view.line.as_ref().is_some_and(|l| l.advice_id == advice_id)
                {
                    self.view.line = None;
                    self.line_until = now;
                }
            }
        }
        self.flush_log();
    }

    fn update_view(&mut self, now: u64, shown: &[Advice]) {
        let hat = self.profile.as_ref().map(|p| p.visual_identity.hat).unwrap_or(Hat::SyrupCap);
        self.view.hat = hat;
        self.view.reduced_motion = self.cfg.reduced_motion;
        if let Some(a) = shown.first() {
            let words = a.text.split_whitespace().count() as u64;
            self.line_until = now + (4000 + words * 350).min(12_000);
            self.view.line =
                Some(Line { advice_id: a.id, topic: a.topic.clone(), text: a.text.clone(), source: source_label(a) });
            self.view.expression = a.expression;
            self.view.speaking = true;
        } else if now >= self.line_until {
            self.view.line = None;
            self.view.speaking = false;
            self.view.expression = if self.research_pending() > 0 {
                Expression::Researching
            } else if self.identity.as_ref().is_none_or(|i| !i.is_confident()) {
                Expression::Thinking
            } else {
                Expression::Neutral
            };
        } else {
            self.view.speaking = now < self.line_until.saturating_sub(2000);
        }
        self.view.status = match &self.identity {
            None => "Looking at the screen.".into(),
            Some(i) if !i.is_known() && !i.is_confident() => format!("New game ({}). Watching and learning.", i.title),
            Some(i) if !i.is_confident() => format!("Maybe {}? Not sure yet.", i.title),
            Some(i) => {
                let known: Vec<&str> = self.state.state().concepts.keys().map(|s| s.as_str()).take(4).collect();
                if known.is_empty() {
                    format!("Watching {}.", i.title)
                } else {
                    format!("Watching {}. I see: {}.", i.title, known.join(", ").replace('_', " "))
                }
            }
        };
        if self.view.mode == OverlayMode::Analysis {
            self.view.analysis = self.analysis_lines();
        }
    }

    fn analysis_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        match &self.identity {
            Some(i) => out.push(format!(
                "game: {} ({}, {})",
                i.title,
                i.confidence,
                if i.is_known() {
                    i.game_id.as_str()
                } else if i.is_confident() {
                    "learned"
                } else {
                    "new"
                }
            )),
            None => out.push("game: not sure yet".into()),
        }
        if let Some(o) = &self.last_obs {
            out.push(format!(
                "scene: {} {} · {} regions · {} texts",
                o.scene.kind.word(),
                o.scene.confidence,
                o.ui_regions.len(),
                o.text.len()
            ));
        }
        out.push(format!(
            "{:.1} fps seen, {:.1} analysed · {:.1} ms",
            self.fps.captured,
            self.fps.analysed,
            self.timings.get("total").copied().unwrap_or(0.0)
        ));
        let mut concepts: Vec<_> = self.state.state().concepts.values().collect();
        concepts.sort_by(|a, b| b.confidence.value().total_cmp(&a.confidence.value()));
        for c in concepts.into_iter().take(8) {
            out.push(format!("{}: {} ({})", c.name.replace('_', " "), concept_text(c), c.confidence));
        }
        if self.research_pending() > 0 {
            out.push(format!("researching ({} pending)", self.research_pending()));
        }
        out
    }

    fn publish_snapshot(&self, frame: &Frame, obs: &Observation) {
        let game_id = self.game_id();
        let snap = Snapshot {
            session: self.session_id.clone(),
            t_ms: frame.timestamp_ms,
            game: self.identity.clone(),
            candidates: self.candidates.iter().take(5).cloned().collect(),
            plugin: self.plugins.active_id().map(|s| s.to_string()),
            ocr: self.analyzer.ocr_engine().to_string(),
            research: self.research.is_some(),
            scene: Some(SceneSummary {
                kind: obs.scene.kind.word().into(),
                confidence: obs.scene.confidence.value(),
                reason: obs.scene.reason.clone(),
            }),
            state: self.state.state().clone(),
            profile: self.profile.as_ref().map(|p| ProfileSummary {
                game_id: p.game_id.clone(),
                title: p.title.clone(),
                genres: p.genres.clone(),
                hat: p.visual_identity.hat.name().into(),
                sessions: p.stats.sessions,
                observed_min: p.stats.observed_ms as f64 / 60_000.0,
                elements: p.known_ui_elements.clone(),
                terms: p.top_terms(30).into_iter().map(|(t, n)| (t.to_string(), n)).collect(),
                version: p.current_version.clone(),
            }),
            player: self.player.games.get(&game_id).cloned(),
            facts: self
                .knowledge
                .as_ref()
                .map(|g| g.facts.iter().rev().take(40).cloned().collect())
                .unwrap_or_default(),
            researched: self.knowledge.as_ref().map(|g| g.researched.clone()).unwrap_or_default(),
            advice: self.advice_log.iter().rev().take(60).rev().cloned().collect(),
            timings: self.timings.clone(),
            fps: self.fps,
            regions: obs.ui_regions.len(),
            texts: obs.text.iter().map(|t| t.text.clone()).collect(),
            uncertainties: obs.uncertainties.iter().map(|u| format!("{}: {}", u.about, u.reason)).collect(),
            view: Some(self.view.clone()),
        };
        if let Ok(mut s) = self.shared.lock() {
            s.snapshot = snap;
            s.frame = Some(frame.image.clone());
            s.observation = Some(obs.clone());
        }
    }

    /// Writes the profile, knowledge and player model to disk.
    pub fn save_all(&mut self) {
        if let Some(p) = &self.profile {
            let _ = self.memory.save_profile(p);
        }
        if let Some(g) = &self.knowledge {
            let _ = self.memory.save(&g.game_id, "knowledge.json", g);
        }
        let _ = self.memory.save_player(&self.cfg.player_id, &self.player);
    }

    /// Ends the session: the summary (shown in post-game mode), everything saved.
    pub fn finish(&mut self) -> SessionSummary {
        // Research still on its way: a short wait, then on without it.
        if let Some(w) = self.research.as_mut()
            && w.pending() > 0
        {
            let done = w.wait(std::time::Duration::from_secs(3));
            let _ = done;
        }
        let now = self.last_ms;
        let minutes = (self.last_ms.saturating_sub(self.first_ms.unwrap_or(self.last_ms))) as f64 / 60_000.0;
        let title = self.identity.as_ref().map(|i| i.title.clone()).unwrap_or_else(|| "This session".into());
        let game_id = self.game_id();
        if let Some(g) = self.player.games.get_mut(&game_id) {
            g.sessions += 1;
        }
        let summary_advice = self.coach.summary(
            now,
            &title,
            self.player.games.get(&game_id),
            self.counters.deaths,
            minutes,
            &self.counters.learned,
        );
        self.view.mode = OverlayMode::PostGame;
        self.view.summary =
            summary_advice.text.split(". ").map(|s| s.trim_end_matches('.').to_string() + ".").collect();
        self.view.line = None;
        self.view.expression = Expression::Proud;
        self.publish(Event::AdviceShown {
            ts_ms: now,
            advice_id: summary_advice.id,
            text: summary_advice.text.clone(),
        });
        self.record_advice(summary_advice.clone(), true, None, now);
        self.save_all();
        self.flush_log();
        let summary = SessionSummary {
            session: self.session_id.clone(),
            game_id: self.identity.as_ref().map(|i| i.game_id.clone()),
            title: self.identity.as_ref().map(|i| i.title.clone()),
            started: self.started_at.clone(),
            ended: now_iso(),
            duration_s: minutes * 60.0,
            analysed_frames: self.analyzer.analysed(),
            deaths: self.counters.deaths,
            victories: self.counters.victories,
            level_ups: self.counters.level_ups,
            advice_shown: self.counters.shown,
            advice_suppressed: self.counters.suppressed,
            feedback: self.counters.feedback.clone(),
            concepts: self.counters.learned.clone(),
            highlights: self.player.games.get(&game_id).map(|g| g.highlights()).unwrap_or_default(),
        };
        if !self.finished {
            self.finished = true;
            if let Some(log) = self.log.take() {
                let _ = log.finish(&summary);
            }
        }
        summary
    }
}

/// True at most once every 10 seconds per topic.
fn once_in_a_while(last: &mut BTreeMap<String, u64>, topic: &str, now: u64) -> bool {
    let due = last.get(topic).is_none_or(|t| now.saturating_sub(*t) >= 10_000);
    if due {
        last.insert(topic.to_string(), now);
    }
    due
}

fn syrup_kind(k: &UiKind) -> &'static str {
    match k {
        UiKind::Bar { .. } => "bar",
        UiKind::Minimap => "minimap",
        UiKind::TextPanel => "text_panel",
        UiKind::Icon => "icon",
        UiKind::Panel => "panel",
        UiKind::Unknown => "element",
    }
}

/// A concept's value for people: `71%`, `71% (51000/71867)`, `12/30`, `1:05`, `Mossy Hills`.
pub fn concept_text(c: &syrup_core::ConceptValue) -> String {
    use syrup_core::state::ConceptUnit;
    match (c.unit, c.value) {
        (ConceptUnit::Fraction, Some(v)) => match c.max {
            Some(m) if m > 1.0 => format!("{:.0}% ({:.0}/{m:.0})", v * 100.0, v * m),
            _ => format!("{:.0}%", v * 100.0),
        },
        (ConceptUnit::Seconds, Some(v)) => {
            let s = v.max(0.0).round() as u64;
            format!("{}:{:02}", s / 60, s % 60)
        }
        (_, Some(v)) => match c.max {
            Some(m) if m > v => format!("{v}/{m}"),
            _ => format!("{v}"),
        },
        _ => c.text.clone().unwrap_or_else(|| "?".into()),
    }
}

/// The game a title names: a catalogued game's id and proper title, or an id
/// made from the title for a game Syrup does not know.
pub fn game_for_title(title: &str) -> (String, String) {
    let key = normalize_words(title);
    let named = |s: &str| normalize_words(s) == key;
    match syrup_recognition::CATALOG
        .iter()
        .find(|e| named(e.id) || named(e.title) || e.aliases.iter().any(|a| named(a)))
    {
        Some(e) => (e.id.to_string(), e.title.to_string()),
        None => (syrup_core::knowledge::node_id(title), title.trim().to_string()),
    }
}

/// "wiki · 76%", "I saw it · 90%", "MapleStory plugin · 70%".
pub fn source_label(a: &Advice) -> String {
    let conf = format!("{:.0}%", a.confidence.value() * 100.0);
    let from = if a.origin.starts_with("plugin:") {
        format!("{} plugin", a.origin.trim_start_matches("plugin:"))
    } else if a.origin == "knowledge" {
        match a.rests_on.first() {
            Some(FactKind::CurrentPatch) => "patch notes".into(),
            Some(FactKind::CommunityConsensus) => "players say".into(),
            Some(FactKind::Speculation) => "a rumour".into(),
            _ => "looked up".into(),
        }
    } else if a.origin == "explain" {
        "why".into()
    } else {
        "I saw it".into()
    };
    format!("{from} · {conf}")
}

#[cfg(test)]
mod tests;
