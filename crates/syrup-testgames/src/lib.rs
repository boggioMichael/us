//! Three synthetic games that know exactly what they show.
//!
//! Each game plays itself (a scripted autopilot with a seeded random
//! generator), draws every frame, and reports the ground truth for it: where
//! each piece of interface is, what it means, its value, which screen is up,
//! and what just happened. Perception, tracking, recognition, profiles and
//! coaching are measured against that truth instead of being eyeballed.
//!
//! - [`dungeon::Dungeon`]: a first-person 3D dungeon (a raycaster): a health
//!   bar, ammo and gold counters, a minimap, deaths, a level-complete screen.
//! - [`scroller::Scroller`]: a 2D side-scroller in the MMO style: HP, MP and
//!   EXP bars, a level, a minimap panel, chat, a quest tracker, a boss with
//!   its bar, level-ups, a death dialog.
//! - [`cards::Cards`]: a card game: menu, table, score, rounds, a turn timer,
//!   victory and defeat screens.
//!
//! The games are fictional and drawn from shapes and DejaVu text; nothing in
//! them comes from a real game.

pub mod cards;
pub mod dungeon;
pub mod hud;
pub mod rng;
pub mod scroller;

use std::sync::{Arc, Mutex};

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use syrup_capture::{Capture, CaptureError, FrameSource};
use syrup_core::frame::SourceKind;
use syrup_core::{Frame, Rect, SceneKind, SourceInfo};

/// One piece of interface as the game drew it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TruthElement {
    /// The game's own name for it (`hp`, `ammo`, `minimap`).
    pub name: String,
    /// What a player would call it (`health`, `ammo`, `currency`, `minimap`...).
    pub concept: String,
    /// `bar`, `text`, `minimap`, `panel`.
    pub kind: String,
    pub rect: Rect,
    pub value: Option<f64>,
    pub max: Option<f64>,
    /// The text drawn, when it is text.
    pub text: Option<String>,
    pub color: Option<[u8; 3]>,
}

impl TruthElement {
    pub fn fraction(&self) -> Option<f64> {
        match (self.value, self.max) {
            (Some(v), Some(m)) if m > 0.0 => Some(v / m),
            _ => None,
        }
    }
}

/// Everything true about one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Truth {
    pub t_ms: u64,
    pub scene: SceneKind,
    pub elements: Vec<TruthElement>,
    /// Where the player's character is on screen (none in first person).
    pub player: Option<Rect>,
}

impl Truth {
    pub fn element(&self, name: &str) -> Option<&TruthElement> {
        self.elements.iter().find(|e| e.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TruthEventKind {
    SceneChanged { to: SceneKind },
    Died,
    Respawned,
    Victory,
    Defeat,
    LevelUp { level: u32 },
    ObjectiveChanged { text: String },
    Healed { amount: f64 },
    EnemyDefeated,
    BossAppeared { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TruthEvent {
    pub t_ms: u64,
    #[serde(flatten)]
    pub kind: TruthEventKind,
}

/// A game that plays itself.
pub trait Game: Send {
    /// The window title and executable a real game would have.
    fn info(&self) -> SourceInfo;
    fn size(&self) -> (u32, u32);
    /// Advances the game (and its autopilot) by `dt_ms`.
    fn update(&mut self, dt_ms: u64);
    fn render(&self) -> RgbaImage;
    fn truth(&self) -> Truth;
    /// What happened since the last call.
    fn drain_events(&mut self) -> Vec<TruthEvent>;
    fn time_ms(&self) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameKind {
    Dungeon,
    Scroller,
    Cards,
}

impl GameKind {
    pub const ALL: [GameKind; 3] = [GameKind::Dungeon, GameKind::Scroller, GameKind::Cards];

    pub fn parse(s: &str) -> Option<GameKind> {
        match s.to_ascii_lowercase().as_str() {
            "dungeon" | "dungeon3d" | "3d" | "fps" => Some(GameKind::Dungeon),
            "scroller" | "skymeadow" | "sky-meadow" | "2d" | "mmo" | "platformer" => Some(GameKind::Scroller),
            "cards" | "card" | "highcard" | "high-card" => Some(GameKind::Cards),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GameKind::Dungeon => "dungeon",
            GameKind::Scroller => "scroller",
            GameKind::Cards => "cards",
        }
    }

    pub fn make(self, seed: u64) -> Box<dyn Game> {
        match self {
            GameKind::Dungeon => Box::new(dungeon::Dungeon::new(seed)),
            GameKind::Scroller => Box::new(scroller::Scroller::new(seed)),
            GameKind::Cards => Box::new(cards::Cards::new(seed)),
        }
    }
}

/// The truth for every frame a session produced, and every event.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TruthLog {
    pub frames: Vec<(u64, Truth)>,
    pub events: Vec<TruthEvent>,
}

impl TruthLog {
    /// The truth for the frame taken at `t_ms` (or the last one before it).
    pub fn at(&self, t_ms: u64) -> Option<&Truth> {
        let i = self.frames.partition_point(|(_, t)| t.t_ms <= t_ms);
        self.frames.get(i.checked_sub(1)?).map(|(_, t)| t)
    }
}

/// A game played as a frame source: `frames` frames at `fps`, with the truth
/// kept in a log the caller can read afterwards.
pub struct Session {
    game: Box<dyn Game>,
    fps: f32,
    index: u64,
    frames: Option<u64>,
    info: Arc<SourceInfo>,
    log: Arc<Mutex<TruthLog>>,
    dt_acc: f64,
}

impl Session {
    pub fn new(game: Box<dyn Game>, fps: f32, seconds: Option<f32>) -> Self {
        let mut info = game.info();
        info.kind = SourceKind::Synthetic;
        Session {
            game,
            fps: fps.clamp(1.0, 60.0),
            index: 0,
            frames: seconds.map(|s| (s * fps).round() as u64),
            info: Arc::new(info),
            log: Arc::new(Mutex::new(TruthLog::default())),
            dt_acc: 0.0,
        }
    }

    pub fn of(kind: GameKind, seed: u64, fps: f32, seconds: Option<f32>) -> Self {
        Session::new(kind.make(seed), fps, seconds)
    }

    /// A handle to the truth log, readable while and after the session runs.
    pub fn truth(&self) -> Arc<Mutex<TruthLog>> {
        self.log.clone()
    }

    /// Advances the game one frame and returns the picture (for showing it in a window).
    pub fn step_image(&mut self) -> (RgbaImage, Truth) {
        let dt = 1000.0 / self.fps as f64 + self.dt_acc;
        let whole = dt.floor();
        self.dt_acc = dt - whole;
        if self.index > 0 {
            self.game.update(whole as u64);
        }
        let image = self.game.render();
        let truth = self.game.truth();
        let events = self.game.drain_events();
        if let Ok(mut log) = self.log.lock() {
            log.frames.push((self.index, truth.clone()));
            log.events.extend(events);
        }
        self.index += 1;
        (image, truth)
    }
}

impl FrameSource for Session {
    fn info(&self) -> Arc<SourceInfo> {
        self.info.clone()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        if self.frames.is_some_and(|n| self.index >= n) {
            return Ok(Capture::Ended);
        }
        let index = self.index;
        let (image, truth) = self.step_image();
        Ok(Capture::Frame(Frame::new(index, truth.t_ms, image, self.info.clone())))
    }

    fn nominal_fps(&self) -> f32 {
        self.fps
    }

    fn len_hint(&self) -> Option<u64> {
        self.frames
    }

    fn is_live(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_is_deterministic_and_reports_truth() {
        for kind in GameKind::ALL {
            let mut a = Session::of(kind, 7, 5.0, Some(6.0));
            let mut b = Session::of(kind, 7, 5.0, Some(6.0));
            let mut frames = 0;
            loop {
                match (a.next().unwrap(), b.next().unwrap()) {
                    (Capture::Frame(fa), Capture::Frame(fb)) => {
                        assert_eq!(fa.timestamp_ms, fb.timestamp_ms);
                        assert!(fa.image.as_raw() == fb.image.as_raw(), "{kind:?} frame {frames} differs");
                        frames += 1;
                    }
                    (Capture::Ended, Capture::Ended) => break,
                    _ => panic!("sessions diverged"),
                }
            }
            assert_eq!(frames, 30);
            let log = a.truth();
            let log = log.lock().unwrap();
            assert_eq!(log.frames.len(), 30);
            assert!(log.at(3000).is_some());
        }
    }
}
