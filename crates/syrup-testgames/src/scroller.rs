//! "Sky Meadow Online": a 2D side-scroller in the style of an online RPG.
//!
//! The autopilot walks the meadow, fights slimes (damage numbers pop up),
//! drinks potions when low, levels up, follows a quest, and meets the Mossy
//! King, a boss with its own bar at the top of the screen, who usually wins
//! the first time. The camera follows the player, so the scene scrolls while
//! the interface stays put: a status bar with the level, HP, MP and EXP; a
//! minimap panel; a quest tracker; a chat box.

use image::RgbaImage;
use syrup_core::frame::SourceKind;
use syrup_core::{Rect, SceneKind, SourceInfo};
use syrup_paint::{FontStyle, Painter, measure, rgb, rgba};

use crate::hud;
use crate::rng::Rng;
use crate::{Game, Truth, TruthElement, TruthEvent, TruthEventKind};

pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 540;
const WORLD_W: f32 = 3200.0;
const GROUND: f32 = 440.0;
const LOADING_UNTIL: u64 = 3000;
const PLATFORMS: [(f32, f32, f32); 5] = [
    (500.0, 340.0, 180.0),
    (900.0, 300.0, 220.0),
    (1400.0, 350.0, 160.0),
    (1900.0, 290.0, 200.0),
    (2500.0, 330.0, 240.0),
];

const HP_BAR: Rect = Rect { x: 180, y: 494, w: 240, h: 18 };
const MP_BAR: Rect = Rect { x: 430, y: 494, w: 240, h: 18 };
const EXP_BAR: Rect = Rect { x: 0, y: 528, w: 960, h: 12 };
const BOSS_BAR: Rect = Rect { x: 280, y: 58, w: 400, h: 18 };
const MINIMAP: Rect = Rect { x: 8, y: 8, w: 230, h: 118 };
const QUEST: Rect = Rect { x: 700, y: 8, w: 252, h: 60 };
const CHAT: Rect = Rect { x: 8, y: 392, w: 360, h: 86 };
const HP_COLOR: [u8; 3] = [220, 50, 60];
const MP_COLOR: [u8; 3] = [50, 110, 230];
const EXP_COLOR: [u8; 3] = [240, 200, 60];
const BOSS_COLOR: [u8; 3] = [150, 60, 200];

const CHAT_LINES: [&str; 12] = [
    "[All] Mika: anyone for the boss?",
    "[All] Toph: selling mossy gems 5k ea",
    "[Party] Rin: brb 2 min",
    "[All] Juno: how do I get to the harbor",
    "[All] Mika: the king hits hard lol",
    "[Guild] Pell: gg everyone",
    "[All] Sasha: lf2m slime quest",
    "[All] Toph: price check on moss capes?",
    "[Party] Rin: back",
    "[All] Kade: dont stand in the green goo",
    "[All] Juno: ty!",
    "[Guild] Pell: event starts at 8",
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Loading,
    Play,
    Dead { until: u64 },
}

#[derive(Debug, Clone)]
struct Mob {
    x: f32,
    hp: i32,
    max_hp: i32,
    dir: f32,
    boss: bool,
    next_attack: u64,
    hurt_until: u64,
}

pub struct Scroller {
    rng: Rng,
    t: u64,
    phase: Phase,
    px: f32,
    py: f32,
    vy: f32,
    facing: f32,
    walk: f32,
    level: u32,
    hp: f32,
    mp: f32,
    exp: f32,
    potions: u32,
    mobs: Vec<Mob>,
    next_attack: u64,
    attacks: u32,
    slash_until: u64,
    next_jump: u64,
    next_potion: u64,
    level_up_until: u64,
    play_started: u64,
    boss_times: Vec<u64>,
    quest: (String, String, u32, u32),
    chat: Vec<String>,
    next_chat: u64,
    chat_i: usize,
    numbers: Vec<(f32, f32, String, u64, bool)>,
    events: Vec<TruthEvent>,
}

impl Scroller {
    pub fn new(seed: u64) -> Self {
        let mut s = Scroller {
            rng: Rng::new(seed ^ 0x5C_0115),
            t: 0,
            phase: Phase::Loading,
            px: 200.0,
            py: GROUND,
            vy: 0.0,
            facing: 1.0,
            walk: 0.0,
            level: 12,
            hp: 1480.0,
            mp: 800.0,
            exp: 45.2,
            potions: 3,
            mobs: Vec::new(),
            next_attack: 0,
            attacks: 0,
            slash_until: 0,
            next_jump: 5000,
            next_potion: 0,
            level_up_until: 0,
            play_started: 0,
            boss_times: vec![45_000, 115_000],
            quest: ("Slime Trouble".into(), "Defeat slimes".into(), 4, 10),
            chat: CHAT_LINES[..3].iter().map(|s| s.to_string()).collect(),
            next_chat: 9000,
            chat_i: 3,
            numbers: Vec::new(),
            events: Vec::new(),
        };
        for _ in 0..5 {
            s.spawn_slime();
        }
        s
    }

    fn max_hp(&self) -> f32 {
        1000.0 + 40.0 * self.level as f32
    }

    fn max_mp(&self) -> f32 {
        500.0 + 25.0 * self.level as f32
    }

    fn emit(&mut self, kind: TruthEventKind) {
        self.events.push(TruthEvent { t_ms: self.t, kind });
    }

    fn scene(&self) -> SceneKind {
        match self.phase {
            Phase::Loading => SceneKind::Loading,
            Phase::Play => SceneKind::Gameplay,
            Phase::Dead { .. } => SceneKind::Defeat,
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

    fn spawn_slime(&mut self) {
        for _ in 0..30 {
            let x = self.rng.range(120.0, WORLD_W - 120.0);
            if (x - self.px).abs() > 260.0 {
                let dir = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
                self.mobs.push(Mob { x, hp: 3, max_hp: 3, dir, boss: false, next_attack: 0, hurt_until: 0 });
                return;
            }
        }
    }

    fn cam(&self) -> f32 {
        (self.px - WIDTH as f32 / 2.0).clamp(0.0, WORLD_W - WIDTH as f32)
    }

    fn quest_text(&self) -> String {
        if self.quest.2 >= self.quest.3 {
            "Quest complete!".to_string()
        } else {
            format!("{} ({}/{})", self.quest.1, self.quest.2, self.quest.3)
        }
    }

    fn gain_exp(&mut self, amount: f32) {
        self.exp += amount;
        while self.exp >= 100.0 {
            self.exp -= 100.0;
            self.level += 1;
            self.hp = self.max_hp();
            self.mp = self.max_mp();
            self.potions += 2;
            self.level_up_until = self.t + 2000;
            let level = self.level;
            self.emit(TruthEventKind::LevelUp { level });
        }
    }

    fn play(&mut self, dt: f32) {
        let now = self.t;
        let play_t = now - self.play_started;
        if let Some(&at) = self.boss_times.first()
            && play_t >= at
            && !self.mobs.iter().any(|m| m.boss)
        {
            self.boss_times.remove(0);
            let x = (self.px + 220.0 * self.facing).clamp(150.0, WORLD_W - 150.0);
            self.mobs.push(Mob {
                x,
                hp: 40,
                max_hp: 40,
                dir: -self.facing,
                boss: true,
                next_attack: now + 1500,
                hurt_until: 0,
            });
            self.emit(TruthEventKind::BossAppeared { name: "Mossy King".into() });
        }
        // Chat.
        if now >= self.next_chat {
            self.chat.push(CHAT_LINES[self.chat_i % CHAT_LINES.len()].to_string());
            self.chat_i += 1;
            if self.chat.len() > 4 {
                self.chat.remove(0);
            }
            self.next_chat = now + self.rng.int(7000, 12_000) as u64;
        }
        // MP regenerates.
        self.mp = (self.mp + 12.0 * dt).min(self.max_mp());
        // Mobs wander; a boss walks to the player.
        let px = self.px;
        for m in self.mobs.iter_mut() {
            if m.boss {
                let d = px - m.x;
                if d.abs() > 60.0 {
                    m.dir = d.signum();
                    m.x += m.dir * 45.0 * dt;
                }
            } else {
                m.x += m.dir * 40.0 * dt;
                if m.x < 100.0 || m.x > WORLD_W - 100.0 {
                    m.dir = -m.dir;
                }
            }
        }
        // Contact damage.
        let mut damage = 0.0;
        let on_ground = self.py >= GROUND - 0.5;
        for i in 0..self.mobs.len() {
            let reach = if self.mobs[i].boss { 70.0 } else { 30.0 };
            if (self.mobs[i].x - self.px).abs() < reach && on_ground && now >= self.mobs[i].next_attack {
                let (lo, hi) = if self.mobs[i].boss { (240.0, 320.0) } else { (60.0, 110.0) };
                damage += self.rng.range(lo, hi);
                self.mobs[i].next_attack = now + if self.mobs[i].boss { 1000 } else { 900 };
            }
        }
        if damage > 0.0 {
            self.hp = (self.hp - damage).max(0.0);
            self.numbers.push((self.px, self.py - 60.0, format!("{}", damage.round() as i32), now + 900, true));
        }
        if self.hp <= 0.0 {
            self.emit(TruthEventKind::Died);
            self.set_phase(Phase::Dead { until: now + 5000 });
            return;
        }
        if self.hp < self.max_hp() * 0.3 && self.potions > 0 && now >= self.next_potion {
            self.potions -= 1;
            let amount = self.max_hp() * 0.45;
            self.hp = (self.hp + amount).min(self.max_hp());
            self.next_potion = now + 3000;
            self.emit(TruthEventKind::Healed { amount: amount as f64 });
        }
        // Fight or walk.
        let target = self
            .mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| (m.x - self.px) * self.facing > -40.0 && (m.x - self.px).abs() < 500.0)
            .min_by(|a, b| (a.1.x - self.px).abs().total_cmp(&(b.1.x - self.px).abs()))
            .map(|(i, m)| (i, m.x));
        match target {
            Some((i, mx)) if (mx - self.px).abs() < if self.mobs[i].boss { 80.0 } else { 55.0 } => {
                self.facing = (mx - self.px).signum().max(-1.0);
                if self.facing == 0.0 {
                    self.facing = 1.0;
                }
                if now >= self.next_attack {
                    self.next_attack = now + 450;
                    self.attacks += 1;
                    self.slash_until = now + 160;
                    let skill = self.attacks.is_multiple_of(4) && self.mp >= 60.0;
                    if skill {
                        self.mp -= 60.0;
                    }
                    let hits = if skill { 2 } else { 1 };
                    let shown = self.rng.int(30, 60) * hits as i64;
                    let m = &mut self.mobs[i];
                    m.hp -= hits;
                    m.hurt_until = now + 150;
                    self.numbers.push((
                        m.x,
                        GROUND - if m.boss { 90.0 } else { 40.0 },
                        format!("{shown}"),
                        now + 800,
                        false,
                    ));
                    if m.hp <= 0 {
                        let boss = m.boss;
                        self.mobs.remove(i);
                        self.emit(TruthEventKind::EnemyDefeated);
                        if boss {
                            self.gain_exp(40.0);
                        } else {
                            let gain = self.rng.range(6.0, 9.0);
                            self.gain_exp(gain);
                            if self.quest.2 < self.quest.3 {
                                self.quest.2 += 1;
                                let text = self.quest_text();
                                self.emit(TruthEventKind::ObjectiveChanged { text });
                            }
                            self.spawn_slime();
                        }
                    }
                }
            }
            Some((_, mx)) => {
                self.facing = if mx >= self.px { 1.0 } else { -1.0 };
                self.step(dt);
            }
            None => self.step(dt),
        }
        // Quest turned in: a new one.
        if self.quest.2 >= self.quest.3 && self.rng.chance(0.004) {
            self.quest = ("Gem Hunter".into(), "Collect mossy gems".into(), 0, 5);
            let text = self.quest_text();
            self.emit(TruthEventKind::ObjectiveChanged { text });
        }
        // Jumps.
        if on_ground && now >= self.next_jump {
            self.vy = -520.0;
            self.next_jump = now + self.rng.int(5000, 9000) as u64;
        }
        self.vy += 1400.0 * dt;
        self.py += self.vy * dt;
        let floor = self.floor_at(self.px);
        if self.vy >= 0.0 && self.py >= floor {
            self.py = floor;
            self.vy = 0.0;
        }
        self.numbers.retain(|n| n.3 > now);
    }

    fn floor_at(&self, x: f32) -> f32 {
        let mut floor = GROUND;
        for (px, py, pw) in PLATFORMS {
            if x >= px && x <= px + pw && self.py <= py + 4.0 {
                floor = floor.min(py);
            }
        }
        floor
    }

    fn step(&mut self, dt: f32) {
        self.px += self.facing * 150.0 * dt;
        self.walk += dt * 10.0;
        if self.px < 60.0 {
            self.px = 60.0;
            self.facing = 1.0;
        }
        if self.px > WORLD_W - 60.0 {
            self.px = WORLD_W - 60.0;
            self.facing = -1.0;
        }
    }

    fn respawn(&mut self) {
        self.hp = self.max_hp() * 0.5;
        self.mp = self.max_mp() * 0.5;
        self.px = 200.0;
        self.py = GROUND;
        self.vy = 0.0;
        self.facing = 1.0;
        self.potions = self.potions.max(2);
        self.mobs.retain(|m| !m.boss);
        self.emit(TruthEventKind::Respawned);
        self.set_phase(Phase::Play);
    }

    // ---- drawing ----

    fn draw_scene(&self, img: &mut RgbaImage) {
        let cam = self.cam();
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        {
            let buf: &mut [u8] = img.as_mut();
            for y in 0..h {
                let t = y as f32 / GROUND;
                let sky = [(120.0 + 90.0 * t) as u8, (180.0 + 55.0 * t) as u8, (240.0 + 5.0 * t).min(255.0) as u8];
                for x in 0..w {
                    let wx_far = x as f32 + cam * 0.2;
                    let far = 250.0 + 35.0 * (wx_far / 110.0).sin() + 18.0 * (wx_far / 47.0 + 1.3).sin();
                    let wx_near = x as f32 + cam * 0.5;
                    let near = 330.0 + 28.0 * (wx_near / 80.0 + 0.7).sin() + 10.0 * (wx_near / 31.0).sin();
                    let yf = y as f32;
                    let c = if yf >= GROUND {
                        let wx = x as f32 + cam;
                        if yf < GROUND + 10.0 {
                            [70, 170 - ((wx as i32 / 6) % 3 * 12) as u8, 60]
                        } else {
                            let brick = ((wx as i32).rem_euclid(48) < 2) || ((y as i32 - GROUND as i32) % 20 < 2);
                            if brick { [90, 60, 40] } else { [128, 88, 56] }
                        }
                    } else if yf >= near {
                        [70, 140, 80]
                    } else if yf >= far {
                        [140, 190, 150]
                    } else {
                        sky
                    };
                    let i = (y * w + x) * 4;
                    buf[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
            }
        }
        let mut p = Painter::new(img);
        // Trees at 0.6 parallax.
        let tree_cam = cam * 0.6;
        let first = ((tree_cam - 100.0) / 170.0).floor() as i32;
        for k in first..first + 8 {
            let x = k as f32 * 170.0 - tree_cam + 40.0 * ((k * 7) % 3) as f32;
            p.fill_rect(x as i32 - 5, 360, 10, 80, rgb(100, 70, 40));
            p.fill_circle(x, 350.0, 36.0, rgb(50, 120, 60));
            p.fill_circle(x - 22.0, 364.0, 24.0, rgb(60, 135, 70));
        }
        // Platforms.
        for (x, y, pw) in PLATFORMS {
            let sx = x - cam;
            p.fill_rect(sx as i32, y as i32, pw as i32, 18, rgb(150, 105, 60));
            p.fill_rect(sx as i32, y as i32, pw as i32, 6, rgb(80, 175, 70));
        }
        // Mobs.
        for m in &self.mobs {
            let sx = m.x - cam;
            if !(-120.0..WIDTH as f32 + 120.0).contains(&sx) {
                continue;
            }
            let bounce = ((self.t as f32 / 180.0 + m.x).sin() * 3.0).abs();
            let (rw, rh) = if m.boss { (58.0, 46.0) } else { (19.0, 14.0) };
            let body = if m.hurt_until > self.t {
                rgb(255, 240, 240)
            } else if m.boss {
                rgb(90, 150, 60)
            } else {
                rgb(110, 200, 90)
            };
            let cy = GROUND - rh - bounce;
            p.fill_ellipse(sx, cy, rw, rh, body);
            p.fill_ellipse(sx - rw * 0.3, cy - rh * 0.5, rw * 0.35, rh * 0.25, rgba(255, 255, 255, 90));
            p.fill_circle(sx - rw * 0.3, cy - rh * 0.1, rw * 0.12, rgb(20, 30, 20));
            p.fill_circle(sx + rw * 0.3, cy - rh * 0.1, rw * 0.12, rgb(20, 30, 20));
            if m.boss {
                // A crown of moss.
                p.fill_polygon(
                    &[
                        (sx - 30.0, cy - 40.0),
                        (sx - 20.0, cy - 64.0),
                        (sx - 8.0, cy - 44.0),
                        (sx, cy - 68.0),
                        (sx + 8.0, cy - 44.0),
                        (sx + 20.0, cy - 64.0),
                        (sx + 30.0, cy - 40.0),
                    ],
                    rgb(220, 190, 60),
                );
            } else if m.hp < m.max_hp {
                let r = Rect::new((sx - 20.0) as i32, (cy - rh - 12.0) as i32, 40, 6);
                p.fill_rect(r.x, r.y, r.w as i32, r.h as i32, rgb(20, 20, 20));
                p.fill_rect(
                    r.x + 1,
                    r.y + 1,
                    ((r.w - 2) as f32 * m.hp as f32 / m.max_hp as f32) as i32,
                    4,
                    rgb(230, 60, 60),
                );
            }
        }
        // The player.
        let sx = self.px - cam;
        let feet = self.py;
        let swing = (self.walk).sin() * 5.0;
        p.fill_rect((sx - 7.0 + swing) as i32, (feet - 14.0) as i32, 6, 14, rgb(60, 50, 90));
        p.fill_rect((sx + 1.0 - swing) as i32, (feet - 14.0) as i32, 6, 14, rgb(60, 50, 90));
        p.fill_rounded_rect(sx - 11.0, feet - 34.0, 22.0, 22.0, 5.0, rgb(60, 110, 200));
        p.fill_circle(sx, feet - 42.0, 11.0, rgb(250, 220, 190));
        p.fill_ellipse(sx, feet - 50.0, 12.0, 6.0, rgb(120, 70, 40));
        p.fill_circle(sx + 4.0 * self.facing, feet - 42.0, 1.8, rgb(30, 30, 30));
        if self.t < self.slash_until {
            let fx = sx + 26.0 * self.facing;
            p.stroke_circle(fx, feet - 30.0, 22.0, 5.0, rgba(255, 250, 200, 220));
        }
        // Damage numbers.
        for (x, y, text, until, hurt) in self.numbers.iter().filter(|n| n.3 > self.t) {
            let age = 1.0 - until.saturating_sub(self.t) as f32 / 900.0;
            let color = if *hurt { rgb(200, 120, 255) } else { rgb(255, 150, 40) };
            p.text_outlined(x - cam - 12.0, y - age * 30.0, text, FontStyle::bold(20.0), color, rgb(40, 20, 0));
        }
        if self.t < self.level_up_until {
            hud::banner(&mut p, sx, feet - 110.0, "LEVEL UP!", 34.0, rgb(255, 220, 80), rgb(90, 50, 0));
        }
    }

    fn hp_text(&self) -> String {
        format!("HP {}/{}", self.hp.ceil() as i32, self.max_hp() as i32)
    }

    fn mp_text(&self) -> String {
        format!("MP {}/{}", self.mp.floor() as i32, self.max_mp() as i32)
    }

    fn exp_text(&self) -> String {
        format!("EXP {:.2}%", self.exp)
    }

    fn level_text(&self) -> String {
        format!("Lv. {}", self.level)
    }

    fn draw_hud(&self, p: &mut Painter) {
        let cam = self.cam();
        // Status bar.
        p.gradient_rect(0, 486, WIDTH as i32, 54, rgb(34, 34, 50), rgb(14, 14, 22));
        p.fill_rect(0, 486, WIDTH as i32, 2, rgb(96, 96, 130));
        hud::label(p, 14.0, 497.0, &self.level_text(), 20.0, rgb(250, 215, 90));
        p.text(92.0, 501.0, "Syrupfan", FontStyle::bold(13.0), rgb(235, 235, 245));
        hud::bar(p, HP_BAR, self.hp / self.max_hp(), rgb(HP_COLOR[0], HP_COLOR[1], HP_COLOR[2]), rgb(80, 80, 96));
        hud::bar(p, MP_BAR, self.mp / self.max_mp(), rgb(MP_COLOR[0], MP_COLOR[1], MP_COLOR[2]), rgb(80, 80, 96));
        let small = FontStyle::bold(11.0);
        for (r, text) in [(HP_BAR, self.hp_text()), (MP_BAR, self.mp_text())] {
            p.text_outlined(
                r.x as f32 + (r.w as f32 - measure(&text, small)) / 2.0,
                r.y as f32 + 3.0,
                &text,
                small,
                rgb(255, 255, 255),
                rgb(0, 0, 0),
            );
        }
        hud::label(p, 690.0, 497.0, &self.exp_text(), 16.0, rgb(250, 225, 110));
        hud::bar(p, EXP_BAR, self.exp / 100.0, rgb(EXP_COLOR[0], EXP_COLOR[1], EXP_COLOR[2]), rgb(50, 50, 60));
        // Minimap.
        p.fill_rect(MINIMAP.x, MINIMAP.y, MINIMAP.w as i32, MINIMAP.h as i32, rgba(16, 20, 34, 225));
        p.fill_rect(MINIMAP.x, MINIMAP.y, MINIMAP.w as i32, 20, rgba(60, 70, 110, 240));
        p.text(
            MINIMAP.x as f32 + 8.0,
            MINIMAP.y as f32 + 3.0,
            "Mossy Hills",
            FontStyle::bold(13.0),
            rgb(240, 240, 255),
        );
        let (mx0, my0, mw, mh) =
            (MINIMAP.x as f32 + 6.0, MINIMAP.y as f32 + 26.0, MINIMAP.w as f32 - 12.0, MINIMAP.h as f32 - 32.0);
        let to_map = |x: f32, y: f32| (mx0 + x / WORLD_W * mw, my0 + (y - 200.0) / 260.0 * mh);
        let (g0, gy) = to_map(0.0, GROUND);
        p.fill_rect(g0 as i32, gy as i32, mw as i32, 2, rgb(120, 200, 110));
        for (x, y, pw) in PLATFORMS {
            let (a, b) = to_map(x, y);
            p.fill_rect(a as i32, b as i32, (pw / WORLD_W * mw).max(2.0) as i32, 2, rgb(170, 140, 90));
        }
        for m in &self.mobs {
            let (a, b) = to_map(m.x, GROUND - 10.0);
            p.fill_circle(
                a,
                b,
                if m.boss { 4.0 } else { 2.2 },
                if m.boss { rgb(200, 90, 255) } else { rgb(240, 80, 80) },
            );
        }
        let (a, b) = to_map(self.px, self.py - 10.0);
        p.fill_circle(a, b, 3.0, rgb(255, 230, 60));
        p.stroke_rect(MINIMAP.x, MINIMAP.y, MINIMAP.w as i32, MINIMAP.h as i32, 2, rgb(120, 130, 170));
        // Quest tracker.
        p.fill_rect(QUEST.x, QUEST.y, QUEST.w as i32, QUEST.h as i32, rgba(0, 0, 0, 150));
        p.text(
            QUEST.x as f32 + 10.0,
            QUEST.y as f32 + 8.0,
            &format!("Quest: {}", self.quest.0),
            FontStyle::bold(13.0),
            rgb(250, 210, 90),
        );
        p.text(
            QUEST.x as f32 + 10.0,
            QUEST.y as f32 + 32.0,
            &self.quest_text(),
            FontStyle::bold(13.0),
            rgb(240, 240, 240),
        );
        // Chat.
        p.fill_rect(CHAT.x, CHAT.y, CHAT.w as i32, CHAT.h as i32, rgba(0, 0, 0, 140));
        for (i, line) in self.chat.iter().enumerate() {
            p.text(
                CHAT.x as f32 + 6.0,
                CHAT.y as f32 + 4.0 + i as f32 * 20.0,
                line,
                FontStyle::bold(11.0),
                rgb(235, 235, 235),
            );
        }
        // Boss bar.
        if let Some(boss) = self.mobs.iter().find(|m| m.boss) {
            let sx = boss.x - cam;
            if (-200.0..WIDTH as f32 + 200.0).contains(&sx) {
                p.text_centered(WIDTH as f32 / 2.0, 34.0, "Mossy King", FontStyle::bold(18.0), rgb(245, 235, 255));
                hud::bar(
                    p,
                    BOSS_BAR,
                    boss.hp as f32 / boss.max_hp as f32,
                    rgb(BOSS_COLOR[0], BOSS_COLOR[1], BOSS_COLOR[2]),
                    rgb(40, 30, 50),
                );
            }
        }
    }

    fn boss_visible(&self) -> Option<&Mob> {
        let cam = self.cam();
        self.mobs.iter().find(|m| m.boss && (-200.0..WIDTH as f32 + 200.0).contains(&(m.x - cam)))
    }
}

impl Game for Scroller {
    fn info(&self) -> SourceInfo {
        let mut info =
            SourceInfo::new(SourceKind::Window).with_title("Sky Meadow Online").with_executable("skymeadow.exe");
        info.executable_path = Some("C:\\Program Files\\SkyMeadow\\skymeadow.exe".into());
        info
    }

    fn size(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn update(&mut self, dt_ms: u64) {
        self.t += dt_ms;
        let dt = dt_ms as f32 / 1000.0;
        match self.phase {
            Phase::Loading if self.t >= LOADING_UNTIL => {
                self.play_started = self.t;
                self.set_phase(Phase::Play);
            }
            Phase::Play => self.play(dt),
            Phase::Dead { until } if self.t >= until => self.respawn(),
            _ => {}
        }
    }

    fn render(&self) -> RgbaImage {
        let mut img = RgbaImage::new(WIDTH, HEIGHT);
        match self.phase {
            Phase::Loading => {
                let mut p = Painter::new(&mut img);
                p.gradient_rect(0, 0, WIDTH as i32, HEIGHT as i32, rgb(130, 190, 240), rgb(230, 245, 250));
                hud::banner(
                    &mut p,
                    WIDTH as f32 / 2.0,
                    150.0,
                    "Sky Meadow Online",
                    48.0,
                    rgb(255, 255, 255),
                    rgb(40, 90, 150),
                );
                p.text_centered(WIDTH as f32 / 2.0, 260.0, "Connecting...", FontStyle::bold(20.0), rgb(40, 70, 110));
                let t = (self.t as f32 / LOADING_UNTIL as f32).clamp(0.0, 1.0);
                hud::bar(&mut p, Rect::new(280, 300, 400, 14), t, rgb(250, 200, 80), rgb(40, 70, 110));
            }
            Phase::Play => {
                self.draw_scene(&mut img);
                self.draw_hud(&mut Painter::new(&mut img));
            }
            Phase::Dead { .. } => {
                self.draw_scene(&mut img);
                let mut p = Painter::new(&mut img);
                self.draw_hud(&mut p);
                hud::veil(&mut p, rgb(0, 0, 0), 0.45);
                let r = Rect::new(300, 190, 360, 150);
                hud::panel(&mut p, r, rgb(245, 238, 220), rgb(120, 80, 40));
                p.text_centered(480.0, 210.0, "You have died.", FontStyle::bold(20.0), rgb(60, 40, 30));
                p.text_centered(480.0, 246.0, "Return to the nearest town?", FontStyle::bold(16.0), rgb(60, 40, 30));
                hud::button(&mut p, Rect::new(430, 290, 100, 34), "OK", 16.0, true);
            }
        }
        img
    }

    fn truth(&self) -> Truth {
        let mut elements = Vec::new();
        if self.phase == Phase::Play {
            let bar = |name: &str, concept: &str, rect: Rect, v: f32, max: f32, color: [u8; 3]| TruthElement {
                name: name.into(),
                concept: concept.into(),
                kind: "bar".into(),
                rect,
                value: Some(v as f64),
                max: Some(max as f64),
                text: None,
                color: Some(color),
            };
            let text =
                |name: &str, concept: &str, x: f32, y: f32, s: FontStyle, t: String, v: Option<f64>| TruthElement {
                    name: name.into(),
                    concept: concept.into(),
                    kind: "text".into(),
                    rect: Rect::new(x as i32, y as i32, measure(&t, s).ceil() as u32, s.line_height().ceil() as u32),
                    value: v,
                    max: None,
                    text: Some(t),
                    color: None,
                };
            elements.push(bar("hp", "health", HP_BAR, self.hp.ceil(), self.max_hp(), HP_COLOR));
            elements.push(bar("mp", "mana", MP_BAR, self.mp.floor(), self.max_mp(), MP_COLOR));
            elements.push(bar("exp", "experience", EXP_BAR, self.exp, 100.0, EXP_COLOR));
            let small = FontStyle::bold(11.0);
            let ht = self.hp_text();
            elements.push(text(
                "hp_text",
                "health",
                HP_BAR.x as f32 + (HP_BAR.w as f32 - measure(&ht, small)) / 2.0,
                HP_BAR.y as f32 + 3.0,
                small,
                ht,
                Some(self.hp.ceil() as f64),
            ));
            let mt = self.mp_text();
            elements.push(text(
                "mp_text",
                "mana",
                MP_BAR.x as f32 + (MP_BAR.w as f32 - measure(&mt, small)) / 2.0,
                MP_BAR.y as f32 + 3.0,
                small,
                mt,
                Some(self.mp.floor() as f64),
            ));
            elements.push(text(
                "exp_text",
                "experience",
                690.0,
                497.0,
                FontStyle::bold(16.0),
                self.exp_text(),
                Some(self.exp as f64),
            ));
            elements.push(text(
                "level",
                "level",
                14.0,
                497.0,
                FontStyle::bold(20.0),
                self.level_text(),
                Some(self.level as f64),
            ));
            elements.push(text(
                "quest",
                "objective",
                QUEST.x as f32 + 10.0,
                QUEST.y as f32 + 32.0,
                FontStyle::bold(13.0),
                self.quest_text(),
                Some(self.quest.2 as f64),
            ));
            elements.push(TruthElement {
                name: "minimap".into(),
                concept: "minimap".into(),
                kind: "minimap".into(),
                rect: MINIMAP,
                value: None,
                max: None,
                text: None,
                color: None,
            });
            elements.push(TruthElement {
                name: "chat".into(),
                concept: "chat".into(),
                kind: "panel".into(),
                rect: CHAT,
                value: None,
                max: None,
                text: None,
                color: None,
            });
            if let Some(b) = self.boss_visible() {
                elements.push(bar("boss", "boss_health", BOSS_BAR, b.hp as f32, b.max_hp as f32, BOSS_COLOR));
            }
        }
        let player = (self.phase != Phase::Loading).then(|| {
            let sx = self.px - self.cam();
            Rect::new((sx - 12.0) as i32, (self.py - 54.0) as i32, 24, 54)
        });
        Truth { t_ms: self.t, scene: self.scene(), elements, player }
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
    fn the_meadow_has_fights_levels_and_a_boss() {
        let mut g = Scroller::new(1);
        let (mut died, mut levels, mut boss, mut kills) = (0, 0, 0, 0);
        for _ in 0..(130_000 / 100) {
            g.update(100);
            for e in g.drain_events() {
                match e.kind {
                    TruthEventKind::Died => died += 1,
                    TruthEventKind::LevelUp { .. } => levels += 1,
                    TruthEventKind::BossAppeared { .. } => boss += 1,
                    TruthEventKind::EnemyDefeated => kills += 1,
                    _ => {}
                }
            }
        }
        assert!(kills >= 5, "kills {kills}");
        assert!(levels >= 1, "levels {levels}");
        assert!(boss >= 1, "boss {boss}");
        assert!(died >= 1, "died {died}");
    }
}
