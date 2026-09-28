//! "High Card Duel": a card game with a menu, rounds, a score, a turn timer,
//! and victory and defeat screens.
//!
//! Each round the player has ten seconds to play a card; the higher card
//! wins the round, and the first to three wins the match. The autopilot
//! sometimes dithers until the timer runs out, which loses the round.

use image::RgbaImage;
use syrup_core::frame::SourceKind;
use syrup_core::{Rect, SceneKind, SourceInfo};
use syrup_paint::{Color, FontStyle, Painter, measure, rgb, rgba};

use crate::hud;
use crate::rng::Rng;
use crate::{Game, Truth, TruthElement, TruthEvent, TruthEventKind};

pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 540;
const TURN_MS: u64 = 10_000;
const TIMER_BAR: Rect = Rect {
    x: 780,
    y: 46,
    w: 160,
    h: 10,
};
const TIMER_COLOR: [u8; 3] = [240, 200, 60];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Card {
    rank: u8,
    suit: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Menu {
        until: u64,
    },
    /// Waiting for the player: they will play at `play_at` (maybe after the timer).
    Turn {
        started: u64,
        play_at: u64,
    },
    Reveal {
        until: u64,
        mine: Card,
        theirs: Card,
        outcome: i8,
    },
    End {
        until: u64,
        won: bool,
    },
}

pub struct Cards {
    rng: Rng,
    t: u64,
    phase: Phase,
    hand: Vec<Card>,
    theirs: Vec<Card>,
    score: (u32, u32),
    round: u32,
    chosen: usize,
    events: Vec<TruthEvent>,
}

impl Cards {
    pub fn new(seed: u64) -> Self {
        let mut c = Cards {
            rng: Rng::new(seed ^ 0xCA_4D5),
            t: 0,
            phase: Phase::Menu { until: 4000 },
            hand: Vec::new(),
            theirs: Vec::new(),
            score: (0, 0),
            round: 1,
            chosen: 0,
            events: Vec::new(),
        };
        c.deal();
        c
    }

    fn draw_card(&mut self) -> Card {
        Card {
            rank: self.rng.int(2, 14) as u8,
            suit: self.rng.int(0, 3) as u8,
        }
    }

    fn deal(&mut self) {
        self.hand = (0..5).map(|_| self.draw_card()).collect();
        self.theirs = (0..5).map(|_| self.draw_card()).collect();
    }

    fn emit(&mut self, kind: TruthEventKind) {
        self.events.push(TruthEvent { t_ms: self.t, kind });
    }

    fn scene(&self) -> SceneKind {
        match self.phase {
            Phase::Menu { .. } => SceneKind::Menu,
            Phase::Turn { .. } | Phase::Reveal { .. } => SceneKind::Gameplay,
            Phase::End { won: true, .. } => SceneKind::Victory,
            Phase::End { won: false, .. } => SceneKind::Defeat,
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        let before = self.scene();
        self.phase = phase;
        let after = self.scene();
        if before != after {
            self.emit(TruthEventKind::SceneChanged { to: after });
        }
    }

    fn start_turn(&mut self) {
        let think = self.rng.int(1500, 11_500) as u64;
        self.chosen = self.rng.int(0, self.hand.len() as i64 - 1) as usize;
        self.set_phase(Phase::Turn {
            started: self.t,
            play_at: self.t + think,
        });
    }

    fn seconds_left(&self) -> Option<u64> {
        match self.phase {
            Phase::Turn { started, .. } => {
                Some((TURN_MS.saturating_sub(self.t - started)).div_ceil(1000))
            }
            _ => None,
        }
    }

    fn timer_fraction(&self) -> f32 {
        match self.phase {
            Phase::Turn { started, .. } => {
                1.0 - ((self.t - started) as f32 / TURN_MS as f32).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    fn score_text(&self) -> String {
        format!("SCORE {} - {}", self.score.0, self.score.1)
    }

    fn round_text(&self) -> String {
        format!("ROUND {}", self.round)
    }

    fn timer_text(&self) -> String {
        format!("TIME 0:{:02}", self.seconds_left().unwrap_or(0))
    }

    fn draw_table(&self, p: &mut Painter) {
        p.gradient_rect(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            rgb(22, 96, 54),
            rgb(10, 58, 32),
        );
        p.fill_ellipse(480.0, 270.0, 330.0, 150.0, rgba(255, 255, 255, 14));
        p.stroke_circle(480.0, 270.0, 150.0, 3.0, rgba(255, 255, 255, 30));
    }

    fn draw_hud(&self, p: &mut Painter) {
        hud::label(p, 20.0, 14.0, &self.score_text(), 22.0, rgb(250, 250, 250));
        let rt = self.round_text();
        hud::label(
            p,
            480.0 - measure(&rt, FontStyle::bold(20.0)) / 2.0,
            14.0,
            &rt,
            20.0,
            rgb(240, 220, 150),
        );
        let tt = self.timer_text();
        let low = self.seconds_left().is_some_and(|s| s <= 3);
        let tw = measure(&tt, FontStyle::bold(22.0));
        hud::label(
            p,
            940.0 - tw,
            14.0,
            &tt,
            22.0,
            if low {
                rgb(255, 90, 80)
            } else {
                rgb(250, 250, 250)
            },
        );
        let color = if low {
            rgb(235, 70, 60)
        } else {
            rgb(TIMER_COLOR[0], TIMER_COLOR[1], TIMER_COLOR[2])
        };
        hud::bar(p, TIMER_BAR, self.timer_fraction(), color, rgb(20, 40, 28));
    }

    fn draw_hands(&self, p: &mut Painter, lifted: Option<usize>) {
        for i in 0..self.theirs.len() {
            let x = 480.0 - 5.0 * 36.0 + i as f32 * 72.0;
            card_back(p, x, 76.0);
        }
        for (i, c) in self.hand.iter().enumerate() {
            let x = 480.0 - 5.0 * 40.0 + i as f32 * 80.0 + 4.0;
            let y = if lifted == Some(i) { 380.0 } else { 400.0 };
            card_face(p, x, y, *c);
        }
    }
}

fn suit_color(suit: u8) -> Color {
    if suit == 1 || suit == 2 {
        rgb(200, 30, 40)
    } else {
        rgb(25, 25, 30)
    }
}

fn rank_text(rank: u8) -> String {
    match rank {
        11 => "J".into(),
        12 => "Q".into(),
        13 => "K".into(),
        14 => "A".into(),
        n => n.to_string(),
    }
}

fn suit_shape(p: &mut Painter, cx: f32, cy: f32, s: f32, suit: u8) {
    let c = suit_color(suit);
    match suit {
        // Spade: an upside-down heart with a stem.
        0 => {
            p.fill_circle(cx - s * 0.28, cy + s * 0.05, s * 0.3, c);
            p.fill_circle(cx + s * 0.28, cy + s * 0.05, s * 0.3, c);
            p.fill_polygon(
                &[
                    (cx - s * 0.56, cy - s * 0.02),
                    (cx + s * 0.56, cy - s * 0.02),
                    (cx, cy - s * 0.6),
                ],
                c,
            );
            p.fill_polygon(
                &[
                    (cx, cy + s * 0.1),
                    (cx - s * 0.2, cy + s * 0.6),
                    (cx + s * 0.2, cy + s * 0.6),
                ],
                c,
            );
        }
        // Heart.
        1 => {
            p.fill_circle(cx - s * 0.28, cy - s * 0.15, s * 0.3, c);
            p.fill_circle(cx + s * 0.28, cy - s * 0.15, s * 0.3, c);
            p.fill_polygon(
                &[
                    (cx - s * 0.56, cy - s * 0.08),
                    (cx + s * 0.56, cy - s * 0.08),
                    (cx, cy + s * 0.58),
                ],
                c,
            );
        }
        // Diamond.
        2 => p.fill_polygon(
            &[
                (cx, cy - s * 0.6),
                (cx + s * 0.42, cy),
                (cx, cy + s * 0.6),
                (cx - s * 0.42, cy),
            ],
            c,
        ),
        // Club.
        _ => {
            p.fill_circle(cx, cy - s * 0.28, s * 0.26, c);
            p.fill_circle(cx - s * 0.3, cy + s * 0.08, s * 0.26, c);
            p.fill_circle(cx + s * 0.3, cy + s * 0.08, s * 0.26, c);
            p.fill_polygon(
                &[
                    (cx, cy),
                    (cx - s * 0.18, cy + s * 0.6),
                    (cx + s * 0.18, cy + s * 0.6),
                ],
                c,
            );
        }
    }
}

fn card_face(p: &mut Painter, x: f32, y: f32, c: Card) {
    p.fill_rounded_rect(x + 3.0, y + 4.0, 64.0, 92.0, 7.0, rgba(0, 0, 0, 90));
    p.fill_rounded_rect(x, y, 64.0, 92.0, 7.0, rgb(250, 248, 240));
    p.stroke_rounded_rect(x, y, 64.0, 92.0, 7.0, 1.5, rgb(160, 150, 140));
    p.text(
        x + 6.0,
        y + 4.0,
        &rank_text(c.rank),
        FontStyle::bold(20.0),
        suit_color(c.suit),
    );
    suit_shape(p, x + 32.0, y + 56.0, 24.0, c.suit);
}

fn card_back(p: &mut Painter, x: f32, y: f32) {
    p.fill_rounded_rect(x, y, 64.0, 92.0, 7.0, rgb(250, 248, 240));
    p.fill_rounded_rect(x + 4.0, y + 4.0, 56.0, 84.0, 5.0, rgb(160, 40, 50));
    for i in 0..5 {
        p.stroke_circle(
            x + 32.0,
            y + 46.0,
            6.0 + i as f32 * 6.0,
            1.2,
            rgba(255, 220, 200, 90),
        );
    }
}

impl Game for Cards {
    fn info(&self) -> SourceInfo {
        let mut info = SourceInfo::new(SourceKind::Window)
            .with_title("High Card Duel")
            .with_executable("highcard.exe");
        info.executable_path =
            Some("D:\\SteamLibrary\\steamapps\\common\\High Card Duel\\highcard.exe".into());
        info
    }

    fn size(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn update(&mut self, dt_ms: u64) {
        self.t += dt_ms;
        match self.phase {
            Phase::Menu { until } if self.t >= until => {
                self.score = (0, 0);
                self.round = 1;
                self.deal();
                self.start_turn();
            }
            Phase::Turn { started, play_at } => {
                let timed_out = self.t >= started + TURN_MS;
                if self.t >= play_at || timed_out {
                    let mine = self.hand.remove(self.chosen.min(self.hand.len() - 1));
                    let pick = self.rng.int(0, self.theirs.len() as i64 - 1) as usize;
                    let theirs = self.theirs.remove(pick);
                    let outcome = if timed_out || mine.rank < theirs.rank {
                        -1
                    } else if mine.rank > theirs.rank {
                        1
                    } else {
                        0
                    };
                    match outcome {
                        1 => self.score.0 += 1,
                        -1 => self.score.1 += 1,
                        _ => {}
                    }
                    let c = self.draw_card();
                    self.hand.push(c);
                    let c = self.draw_card();
                    self.theirs.push(c);
                    self.set_phase(Phase::Reveal {
                        until: self.t + 1800,
                        mine,
                        theirs,
                        outcome: if timed_out { -2 } else { outcome },
                    });
                }
            }
            Phase::Reveal { until, .. } if self.t >= until => {
                if self.score.0 >= 3 || self.score.1 >= 3 {
                    let won = self.score.0 >= 3;
                    self.emit(if won {
                        TruthEventKind::Victory
                    } else {
                        TruthEventKind::Defeat
                    });
                    self.set_phase(Phase::End {
                        until: self.t + 4000,
                        won,
                    });
                } else {
                    self.round += 1;
                    self.start_turn();
                }
            }
            Phase::End { until, .. } if self.t >= until => self.set_phase(Phase::Menu {
                until: self.t + 3000,
            }),
            _ => {}
        }
    }

    fn render(&self) -> RgbaImage {
        let mut img = RgbaImage::new(WIDTH, HEIGHT);
        let mut p = Painter::new(&mut img);
        self.draw_table(&mut p);
        match self.phase {
            Phase::Menu { .. } => {
                hud::banner(
                    &mut p,
                    480.0,
                    90.0,
                    "HIGH CARD DUEL",
                    48.0,
                    rgb(250, 240, 220),
                    rgb(20, 50, 30),
                );
                for (i, label) in ["PLAY", "RULES", "QUIT"].iter().enumerate() {
                    hud::button(
                        &mut p,
                        Rect::new(380, 220 + i as i32 * 64, 200, 48),
                        label,
                        22.0,
                        i == 0,
                    );
                }
                p.text(
                    20.0,
                    510.0,
                    "Season 2",
                    FontStyle::regular(13.0),
                    rgb(180, 210, 190),
                );
            }
            Phase::Turn { .. } => {
                self.draw_hands(&mut p, Some(self.chosen));
                p.text_centered(
                    480.0,
                    300.0,
                    "Your turn",
                    FontStyle::bold(20.0),
                    rgb(250, 250, 230),
                );
                self.draw_hud(&mut p);
            }
            Phase::Reveal {
                mine,
                theirs,
                outcome,
                ..
            } => {
                self.draw_hands(&mut p, None);
                card_face(&mut p, 400.0, 200.0, theirs);
                card_face(&mut p, 496.0, 212.0, mine);
                let msg = match outcome {
                    1 => "You win the round!",
                    0 => "Tie!",
                    -2 => "Time's up!",
                    _ => "Opponent wins the round",
                };
                p.text_centered(480.0, 318.0, msg, FontStyle::bold(20.0), rgb(250, 250, 230));
                self.draw_hud(&mut p);
            }
            Phase::End { won, .. } => {
                hud::veil(&mut p, rgb(0, 0, 0), 0.5);
                if won {
                    hud::banner(
                        &mut p,
                        480.0,
                        170.0,
                        "VICTORY",
                        72.0,
                        rgb(250, 210, 80),
                        rgb(80, 50, 0),
                    );
                } else {
                    hud::banner(
                        &mut p,
                        480.0,
                        170.0,
                        "DEFEAT",
                        72.0,
                        rgb(200, 70, 70),
                        rgb(40, 0, 0),
                    );
                }
                p.text_centered(
                    480.0,
                    280.0,
                    &format!("Final score {} - {}", self.score.0, self.score.1),
                    FontStyle::bold(22.0),
                    rgb(240, 240, 240),
                );
            }
        }
        img
    }

    fn truth(&self) -> Truth {
        let mut elements = Vec::new();
        if matches!(self.phase, Phase::Turn { .. } | Phase::Reveal { .. }) {
            let text = |name: &str, concept: &str, x: f32, y: f32, size: f32, t: String, v: f64| {
                let s = FontStyle::bold(size);
                TruthElement {
                    name: name.into(),
                    concept: concept.into(),
                    kind: "text".into(),
                    rect: Rect::new(
                        x as i32,
                        y as i32,
                        measure(&t, s).ceil() as u32,
                        s.line_height().ceil() as u32,
                    ),
                    value: Some(v),
                    max: None,
                    text: Some(t),
                    color: None,
                }
            };
            elements.push(text(
                "score",
                "score",
                20.0,
                14.0,
                22.0,
                self.score_text(),
                self.score.0 as f64,
            ));
            let rt = self.round_text();
            elements.push(text(
                "round",
                "round",
                480.0 - measure(&rt, FontStyle::bold(20.0)) / 2.0,
                14.0,
                20.0,
                rt,
                self.round as f64,
            ));
            let tt = self.timer_text();
            let tw = measure(&tt, FontStyle::bold(22.0));
            elements.push(text(
                "timer",
                "timer",
                940.0 - tw,
                14.0,
                22.0,
                tt,
                self.seconds_left().unwrap_or(0) as f64,
            ));
            elements.push(TruthElement {
                name: "timer_bar".into(),
                concept: "timer".into(),
                kind: "bar".into(),
                rect: TIMER_BAR,
                value: Some(self.timer_fraction() as f64 * 10.0),
                max: Some(10.0),
                text: None,
                color: Some(TIMER_COLOR),
            });
        }
        Truth {
            t_ms: self.t,
            scene: self.scene(),
            elements,
            player: None,
        }
    }

    fn drain_events(&mut self) -> Vec<TruthEvent> {
        std::mem::take(&mut self.events)
    }

    fn time_ms(&self) -> u64 {
        self.t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_end_in_victory_or_defeat_and_start_over() {
        let mut g = Cards::new(5);
        let mut ends = 0;
        let mut scenes = Vec::new();
        for _ in 0..(120_000 / 100) {
            g.update(100);
            for e in g.drain_events() {
                match e.kind {
                    TruthEventKind::Victory | TruthEventKind::Defeat => ends += 1,
                    TruthEventKind::SceneChanged { to } => scenes.push(to),
                    _ => {}
                }
            }
        }
        assert!(ends >= 1, "{scenes:?}");
        assert!(scenes.contains(&SceneKind::Menu));
    }
}
