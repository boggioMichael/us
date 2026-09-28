//! "Dungeon 3D": a first-person crawler drawn by a raycaster.
//!
//! The autopilot walks a route through a small maze, turns to face enemies
//! and shoots them (gold for each), picks up health and ammo, gets ambushed
//! and dies now and then, and clears the level after two minutes. The HUD:
//! an HP bar with its number bottom left, ammo bottom right, gold top left, a
//! minimap top right, a crosshair.

use std::collections::VecDeque;

use image::RgbaImage;
use syrup_core::frame::SourceKind;
use syrup_core::{Rect, SceneKind, SourceInfo};
use syrup_paint::{FontStyle, Painter, measure, rgb, rgba};

use crate::hud;
use crate::rng::Rng;
use crate::{Game, Truth, TruthElement, TruthEvent, TruthEventKind};

pub const WIDTH: u32 = 960;
pub const HEIGHT: u32 = 540;

const MAP: [&str; 16] = [
    "################",
    "#......#.......#",
    "#.####.#.#####.#",
    "#.#....#.....#.#",
    "#.#.######.#.#.#",
    "#.#........#...#",
    "#.####.#####.###",
    "#......#.......#",
    "###.##.#.#####.#",
    "#...#..#.#.....#",
    "#.###.##.#.###.#",
    "#.....#..#...#.#",
    "#.###.#.####.#.#",
    "#.#...#......#.#",
    "#.#.########...#",
    "################",
];

const HP_BAR: Rect = Rect {
    x: 58,
    y: 495,
    w: 204,
    h: 24,
};
const HP_COLOR: [u8; 3] = [205, 40, 40];
const MINIMAP: Rect = Rect {
    x: 800,
    y: 12,
    w: 148,
    h: 148,
};
const MENU_UNTIL: u64 = 4000;
const LOADING_UNTIL: u64 = 5500;
const LEVEL_TIME: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Menu,
    Loading,
    Play,
    Dead { until: u64 },
    Complete { until: u64 },
}

#[derive(Debug, Clone)]
struct Enemy {
    x: f32,
    y: f32,
    hp: i32,
    next_attack: u64,
    hurt_until: u64,
}

pub struct Dungeon {
    rng: Rng,
    t: u64,
    phase: Phase,
    walls: Vec<Vec<bool>>,
    x: f32,
    y: f32,
    dir: f32,
    route: VecDeque<(usize, usize)>,
    health: f32,
    ammo: u32,
    gold: u32,
    enemies: Vec<Enemy>,
    next_shot: u64,
    flash_until: u64,
    hurt_until: u64,
    heal_until: u64,
    next_spawn: u64,
    next_heal: u64,
    ammo_at: Option<u64>,
    play_started: u64,
    ambushes: Vec<u64>,
    level: u32,
    popups: Vec<(String, u64, [u8; 3], (f32, f32))>,
    events: Vec<TruthEvent>,
}

impl Dungeon {
    pub fn new(seed: u64) -> Self {
        let walls = MAP
            .iter()
            .map(|row| row.bytes().map(|b| b == b'#').collect())
            .collect();
        let mut d = Dungeon {
            rng: Rng::new(seed ^ 0xD0_0D),
            t: 0,
            phase: Phase::Menu,
            walls,
            x: 1.5,
            y: 1.5,
            dir: 0.0,
            route: VecDeque::new(),
            health: 100.0,
            ammo: 30,
            gold: 0,
            enemies: Vec::new(),
            next_shot: 0,
            flash_until: 0,
            hurt_until: 0,
            heal_until: 0,
            next_spawn: 0,
            next_heal: 0,
            ammo_at: None,
            play_started: 0,
            ambushes: Vec::new(),
            level: 1,
            popups: Vec::new(),
            events: Vec::new(),
        };
        d.extend_route();
        d
    }

    fn wall(&self, x: f32, y: f32) -> bool {
        if x < 0.0 || y < 0.0 {
            return true;
        }
        let (cx, cy) = (x as usize, y as usize);
        self.walls
            .get(cy)
            .and_then(|r| r.get(cx))
            .copied()
            .unwrap_or(true)
    }

    fn open_cells(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (y, row) in self.walls.iter().enumerate() {
            for (x, w) in row.iter().enumerate() {
                if !w {
                    out.push((x, y));
                }
            }
        }
        out
    }

    fn bfs(&self, from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
        let h = self.walls.len();
        let w = self.walls[0].len();
        let mut prev = vec![vec![None; w]; h];
        let mut q = VecDeque::from([from]);
        prev[from.1][from.0] = Some(from);
        while let Some(c) = q.pop_front() {
            if c == to {
                break;
            }
            for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (c.0 as i32 + dx, c.1 as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let n = (nx as usize, ny as usize);
                if !self.walls[n.1][n.0] && prev[n.1][n.0].is_none() {
                    prev[n.1][n.0] = Some(c);
                    q.push_back(n);
                }
            }
        }
        let mut path = Vec::new();
        let mut c = to;
        while c != from {
            path.push(c);
            match prev[c.1][c.0] {
                Some(p) => c = p,
                None => return Vec::new(),
            }
        }
        path.reverse();
        path
    }

    fn extend_route(&mut self) {
        let cells = self.open_cells();
        let mut from = self
            .route
            .back()
            .copied()
            .unwrap_or((self.x as usize, self.y as usize));
        while self.route.len() < 40 {
            let to = cells[self.rng.int(0, cells.len() as i64 - 1) as usize];
            let path = self.bfs(from, to);
            if path.is_empty() {
                continue;
            }
            self.route.extend(path);
            from = to;
        }
    }

    fn emit(&mut self, kind: TruthEventKind) {
        self.events.push(TruthEvent { t_ms: self.t, kind });
    }

    fn set_phase(&mut self, phase: Phase) {
        let before = self.scene();
        self.phase = phase;
        let after = self.scene();
        if before != after {
            self.emit(TruthEventKind::SceneChanged { to: after });
        }
    }

    fn scene(&self) -> SceneKind {
        match self.phase {
            Phase::Menu => SceneKind::Menu,
            Phase::Loading => SceneKind::Loading,
            Phase::Play => SceneKind::Gameplay,
            Phase::Dead { .. } => SceneKind::Defeat,
            Phase::Complete { .. } => SceneKind::Victory,
        }
    }

    fn spawn_enemy_near(&mut self, min_d: f32, max_d: f32) {
        let cells = self.open_cells();
        for _ in 0..60 {
            let (cx, cy) = cells[self.rng.int(0, cells.len() as i64 - 1) as usize];
            let (ex, ey) = (cx as f32 + 0.5, cy as f32 + 0.5);
            let d = (ex - self.x).hypot(ey - self.y);
            if d >= min_d && d <= max_d {
                self.enemies.push(Enemy {
                    x: ex,
                    y: ey,
                    hp: 3,
                    next_attack: self.t + 900,
                    hurt_until: 0,
                });
                return;
            }
        }
    }

    fn angle_to(&self, x: f32, y: f32) -> f32 {
        wrap_angle((y - self.y).atan2(x - self.x) - self.dir)
    }

    /// Distance to the first wall straight ahead along `angle` (absolute).
    fn wall_distance(&self, angle: f32) -> f32 {
        let (dx, dy) = (angle.cos(), angle.sin());
        let mut d = 0.0;
        while d < 20.0 {
            d += 0.05;
            if self.wall(self.x + dx * d, self.y + dy * d) {
                return d;
            }
        }
        20.0
    }

    fn play(&mut self, dt: f32) {
        let now = self.t;
        let play_t = now - self.play_started;
        // Ambushes: several enemies at once, close by.
        if let Some(&at) = self.ambushes.first()
            && play_t >= at
        {
            self.ambushes.remove(0);
            for _ in 0..4 {
                self.spawn_enemy_near(1.5, 3.5);
            }
        }
        if now >= self.next_spawn && self.enemies.len() < 3 {
            self.spawn_enemy_near(4.0, 9.0);
            self.next_spawn = now + self.rng.int(6000, 10_000) as u64;
        }
        // Enemies walk to the player and bite.
        let (px, py) = (self.x, self.y);
        let mut damage = 0.0;
        let mut bites = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let d = (px - e.x).hypot(py - e.y);
            if d < 8.0 && d > 0.7 {
                let (vx, vy) = ((px - e.x) / d, (py - e.y) / d);
                let step = 1.1 * dt;
                let nx = e.x + vx * step;
                if !MAP[e.y as usize].as_bytes()[nx as usize].eq(&b'#') {
                    e.x = nx;
                }
                let ny = e.y + vy * step;
                if !MAP[ny as usize].as_bytes()[e.x as usize].eq(&b'#') {
                    e.y = ny;
                }
            }
            if d <= 0.9 && now >= e.next_attack {
                bites.push(i);
                e.next_attack = now + 800;
            }
        }
        for _ in bites {
            damage += self.rng.range(7.0, 14.0);
        }
        if damage > 0.0 {
            self.health = (self.health - damage).max(0.0);
            self.hurt_until = now + 250;
        }
        if self.health <= 0.0 {
            self.emit(TruthEventKind::Died);
            self.set_phase(Phase::Dead { until: now + 4500 });
            return;
        }
        // Heal and ammo pickups.
        if self.health < 55.0 && now >= self.next_heal {
            self.health = (self.health + 35.0).min(100.0);
            self.heal_until = now + 400;
            self.next_heal = now + 15_000;
            self.popups
                .push(("+35 HP".into(), now + 1200, [90, 230, 110], (300.0, 470.0)));
            self.emit(TruthEventKind::Healed { amount: 35.0 });
        }
        if self.ammo < 4 {
            match self.ammo_at {
                None => self.ammo_at = Some(now + 3000),
                Some(at) if now >= at => {
                    self.ammo += 20;
                    self.ammo_at = None;
                    self.popups.push((
                        "+20 AMMO".into(),
                        now + 1200,
                        [230, 230, 140],
                        (800.0, 470.0),
                    ));
                }
                _ => {}
            }
        }
        // The nearest enemy in view, if any.
        let target = self
            .enemies
            .iter()
            .enumerate()
            .map(|(i, e)| (i, (e.x - px).hypot(e.y - py), self.angle_to(e.x, e.y)))
            .filter(|(_, d, a)| *d < 6.0 && a.abs() < 1.2 && self.wall_distance(self.dir + a) > *d)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, dist, angle)) = target {
            self.turn_towards(angle, dt);
            if angle.abs() < (0.35 / dist).atan().max(0.05)
                && now >= self.next_shot
                && self.ammo > 0
            {
                self.ammo -= 1;
                self.next_shot = now + 380;
                self.flash_until = now + 90;
                let e = &mut self.enemies[i];
                e.hp -= 1;
                e.hurt_until = now + 150;
                if e.hp <= 0 {
                    self.enemies.remove(i);
                    let coins = self.rng.int(10, 25) as u32;
                    self.gold += coins;
                    self.popups.push((
                        format!("+{coins}"),
                        now + 1000,
                        [240, 200, 80],
                        (150.0, 44.0),
                    ));
                    self.emit(TruthEventKind::EnemyDefeated);
                }
            }
            return;
        }
        // Otherwise walk the route.
        if self.route.len() < 10 {
            self.extend_route();
        }
        if let Some(&(cx, cy)) = self.route.front() {
            let (tx, ty) = (cx as f32 + 0.5, cy as f32 + 0.5);
            let angle = self.angle_to(tx, ty);
            self.turn_towards(angle, dt);
            if angle.abs() < 0.35 {
                let step = 1.7 * dt;
                let (nx, ny) = (
                    self.x + self.dir.cos() * step,
                    self.y + self.dir.sin() * step,
                );
                if !self.wall(nx, self.y) {
                    self.x = nx;
                }
                if !self.wall(self.x, ny) {
                    self.y = ny;
                }
            }
            if (tx - self.x).hypot(ty - self.y) < 0.2 {
                self.route.pop_front();
            }
        }
        if play_t >= LEVEL_TIME * self.level as u64 {
            self.emit(TruthEventKind::Victory);
            self.set_phase(Phase::Complete { until: now + 4000 });
        }
    }

    fn turn_towards(&mut self, angle: f32, dt: f32) {
        let max = 2.6 * dt;
        self.dir = wrap_angle(self.dir + angle.clamp(-max, max));
    }

    fn respawn(&mut self) {
        self.health = 100.0;
        self.x = 1.5;
        self.y = 1.5;
        self.dir = 0.0;
        self.enemies.clear();
        self.route.clear();
        self.extend_route();
        self.ammo = self.ammo.max(15);
        self.next_spawn = self.t + 4000;
        self.emit(TruthEventKind::Respawned);
        self.set_phase(Phase::Play);
    }

    // ---- drawing ----

    fn draw_world(&self, img: &mut RgbaImage) {
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let horizon = h / 2;
        let buf: &mut [u8] = img.as_mut();
        // Ceiling and floor.
        for y in 0..h {
            let c = if y < horizon {
                let t = y as f32 / horizon as f32;
                [
                    (18.0 + 24.0 * t) as u8,
                    (18.0 + 22.0 * t) as u8,
                    (28.0 + 24.0 * t) as u8,
                ]
            } else {
                let t = (y - horizon) as f32 / (h - horizon) as f32;
                [
                    (22.0 + 50.0 * t) as u8,
                    (18.0 + 38.0 * t) as u8,
                    (14.0 + 26.0 * t) as u8,
                ]
            };
            for x in 0..w {
                let i = (y * w + x) * 4;
                buf[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let (dx, dy) = (self.dir.cos(), self.dir.sin());
        let (plx, ply) = (-dy * 0.66, dx * 0.66);
        let mut zbuf = vec![f32::MAX; w];
        for (x, z) in zbuf.iter_mut().enumerate() {
            let cam = 2.0 * x as f32 / w as f32 - 1.0;
            let (rx, ry) = (dx + plx * cam, dy + ply * cam);
            let (mut mx, mut my) = (self.x as i32, self.y as i32);
            let ddx = if rx == 0.0 { 1e30 } else { (1.0 / rx).abs() };
            let ddy = if ry == 0.0 { 1e30 } else { (1.0 / ry).abs() };
            let (step_x, mut sdx) = if rx < 0.0 {
                (-1, (self.x - mx as f32) * ddx)
            } else {
                (1, (mx as f32 + 1.0 - self.x) * ddx)
            };
            let (step_y, mut sdy) = if ry < 0.0 {
                (-1, (self.y - my as f32) * ddy)
            } else {
                (1, (my as f32 + 1.0 - self.y) * ddy)
            };
            let mut side = 0;
            for _ in 0..64 {
                if sdx < sdy {
                    sdx += ddx;
                    mx += step_x;
                    side = 0;
                } else {
                    sdy += ddy;
                    my += step_y;
                    side = 1;
                }
                if self
                    .walls
                    .get(my as usize)
                    .and_then(|r| r.get(mx as usize))
                    .copied()
                    .unwrap_or(true)
                {
                    break;
                }
            }
            let dist = if side == 0 { sdx - ddx } else { sdy - ddy }.max(0.05);
            *z = dist;
            let line = (h as f32 / dist) as i32;
            let top = (horizon as i32 - line / 2).max(0);
            let bottom = (horizon as i32 + line / 2).min(h as i32 - 1);
            let wall_x = if side == 0 {
                self.y + dist * ry
            } else {
                self.x + dist * rx
            };
            let wall_x = wall_x - wall_x.floor();
            let fog = 1.0 / (1.0 + dist * 0.22);
            let base = if side == 0 {
                [128.0, 116.0, 104.0]
            } else {
                [98.0, 88.0, 80.0]
            };
            for y in top..=bottom {
                let v = (y - (horizon as i32 - line / 2)) as f32 / line.max(1) as f32;
                let row = (v * 4.0).floor();
                let shift = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
                let bx = (wall_x * 2.0 + shift).fract();
                let mortar = (v * 4.0).fract() < 0.06 || bx < 0.03;
                let k = if mortar {
                    0.45
                } else {
                    1.0 - 0.08 * ((mx * 7 + my * 13 + row as i32) % 3) as f32
                };
                let i = (y as usize * w + x) * 4;
                for c in 0..3 {
                    buf[i + c] = (base[c] * k * fog) as u8;
                }
            }
        }
        // Enemies, far to near, hidden behind walls.
        let inv = 1.0 / (plx * dy - dx * ply);
        let mut order: Vec<&Enemy> = self.enemies.iter().collect();
        order.sort_by(|a, b| {
            (b.x - self.x)
                .hypot(b.y - self.y)
                .total_cmp(&(a.x - self.x).hypot(a.y - self.y))
        });
        for e in order {
            let (sx, sy) = (e.x - self.x, e.y - self.y);
            let tx = inv * (dy * sx - dx * sy);
            let ty = inv * (-ply * sx + plx * sy);
            if ty <= 0.2 {
                continue;
            }
            let screen_x = (w as f32 / 2.0) * (1.0 + tx / ty);
            let size = (h as f32 / ty * 0.62).min(h as f32 * 1.5);
            let top = horizon as f32 + (h as f32 / ty) * 0.5 - size;
            let sprite = enemy_sprite(size.max(4.0) as u32, e.hurt_until > self.t);
            let left = screen_x - size / 2.0;
            let fog = 1.0 / (1.0 + ty * 0.22);
            for sxp in 0..sprite.width() as i32 {
                let col = left as i32 + sxp;
                if col < 0 || col >= w as i32 || ty >= zbuf[col as usize] {
                    continue;
                }
                for syp in 0..sprite.height() as i32 {
                    let row = top as i32 + syp;
                    if row < 0 || row >= h as i32 {
                        continue;
                    }
                    let p = sprite.get_pixel(sxp as u32, syp as u32).0;
                    if p[3] < 128 {
                        continue;
                    }
                    let i = (row as usize * w + col as usize) * 4;
                    for c in 0..3 {
                        buf[i + c] = (p[c] as f32 * (0.35 + 0.65 * fog)) as u8;
                    }
                }
            }
        }
    }

    fn draw_hud(&self, p: &mut Painter) {
        let now = self.t;
        // Crosshair.
        let (cx, cy) = (WIDTH as i32 / 2, HEIGHT as i32 / 2);
        p.fill_rect(cx - 9, cy - 1, 7, 2, rgba(240, 240, 240, 220));
        p.fill_rect(cx + 3, cy - 1, 7, 2, rgba(240, 240, 240, 220));
        p.fill_rect(cx - 1, cy - 9, 2, 7, rgba(240, 240, 240, 220));
        p.fill_rect(cx - 1, cy + 3, 2, 7, rgba(240, 240, 240, 220));
        if now < self.flash_until {
            p.fill_circle(
                WIDTH as f32 / 2.0 + 40.0,
                HEIGHT as f32 - 40.0,
                34.0,
                rgba(255, 220, 120, 200),
            );
            p.fill_circle(
                WIDTH as f32 / 2.0 + 40.0,
                HEIGHT as f32 - 40.0,
                18.0,
                rgba(255, 255, 230, 230),
            );
        }
        // HP.
        hud::label(p, 20.0, 497.0, "HP", 20.0, rgb(245, 240, 235));
        hud::bar(
            p,
            HP_BAR,
            self.health / 100.0,
            rgb(HP_COLOR[0], HP_COLOR[1], HP_COLOR[2]),
            rgb(70, 66, 74),
        );
        hud::label(
            p,
            270.0,
            498.0,
            &self.health_text(),
            18.0,
            rgb(245, 240, 235),
        );
        // Ammo, right aligned.
        let ammo = self.ammo_text();
        let aw = measure(&ammo, FontStyle::bold(20.0));
        hud::label(p, 940.0 - aw, 497.0, &ammo, 20.0, rgb(225, 225, 160));
        // Gold.
        p.fill_circle(28.0, 27.0, 10.0, rgb(230, 180, 40));
        p.stroke_circle(28.0, 27.0, 10.0, 2.0, rgb(150, 105, 20));
        hud::label(p, 46.0, 16.0, &self.gold_text(), 20.0, rgb(240, 200, 80));
        // Minimap.
        p.fill_rect(
            MINIMAP.x,
            MINIMAP.y,
            MINIMAP.w as i32,
            MINIMAP.h as i32,
            rgba(10, 10, 14, 230),
        );
        let cell = (MINIMAP.w as f32 - 4.0) / 16.0;
        for (y, row) in self.walls.iter().enumerate() {
            for (x, wall) in row.iter().enumerate() {
                let c = if *wall {
                    rgb(96, 96, 108)
                } else {
                    rgb(34, 34, 42)
                };
                let (rx, ry) = (
                    MINIMAP.x as f32 + 2.0 + x as f32 * cell,
                    MINIMAP.y as f32 + 2.0 + y as f32 * cell,
                );
                p.fill_rect(
                    rx as i32,
                    ry as i32,
                    cell.ceil() as i32,
                    cell.ceil() as i32,
                    c,
                );
            }
        }
        let to_map = |x: f32, y: f32| {
            (
                MINIMAP.x as f32 + 2.0 + x * cell,
                MINIMAP.y as f32 + 2.0 + y * cell,
            )
        };
        for e in &self.enemies {
            let (mx, my) = to_map(e.x, e.y);
            p.fill_circle(mx, my, 2.6, rgb(230, 50, 50));
        }
        let (mx, my) = to_map(self.x, self.y);
        let a = self.dir;
        let tri = [
            (mx + a.cos() * 6.0, my + a.sin() * 6.0),
            (mx + (a + 2.5).cos() * 4.5, my + (a + 2.5).sin() * 4.5),
            (mx + (a - 2.5).cos() * 4.5, my + (a - 2.5).sin() * 4.5),
        ];
        p.fill_polygon(&tri, rgb(250, 220, 60));
        p.stroke_rect(
            MINIMAP.x,
            MINIMAP.y,
            MINIMAP.w as i32,
            MINIMAP.h as i32,
            2,
            rgb(170, 170, 185),
        );
        // Popups.
        for (text, until, c, (x, y)) in &self.popups {
            if now < *until {
                let rise = (1.0 - (*until - now) as f32 / 1200.0).clamp(0.0, 1.0) * 14.0;
                hud::label(p, *x, *y - rise, text, 18.0, rgb(c[0], c[1], c[2]));
            }
        }
    }

    fn health_text(&self) -> String {
        format!("{}/100", self.health.ceil() as i32)
    }

    fn ammo_text(&self) -> String {
        format!("AMMO {}", self.ammo)
    }

    fn gold_text(&self) -> String {
        format!("GOLD {}", self.gold)
    }

    fn draw_menu(&self, p: &mut Painter) {
        p.gradient_rect(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            rgb(26, 20, 30),
            rgb(8, 6, 10),
        );
        for i in 0..6 {
            let r = 260.0 - i as f32 * 40.0;
            p.fill_circle(WIDTH as f32 / 2.0, 150.0, r, rgba(255, 140, 60, 10));
        }
        hud::banner(
            p,
            WIDTH as f32 / 2.0,
            70.0,
            "DUNGEON 3D",
            72.0,
            rgb(232, 222, 200),
            rgb(40, 20, 10),
        );
        p.text_centered(
            WIDTH as f32 / 2.0,
            160.0,
            "The Crypt of Echoes",
            FontStyle::bold(20.0),
            rgb(200, 170, 130),
        );
        let highlight = self.t >= 1500;
        for (i, label) in ["NEW GAME", "OPTIONS", "QUIT"].iter().enumerate() {
            hud::button(
                p,
                Rect::new(370, 250 + i as i32 * 62, 220, 46),
                label,
                20.0,
                highlight && i == 0,
            );
        }
        p.text(
            860.0,
            515.0,
            "v1.4.2",
            FontStyle::regular(13.0),
            rgb(120, 110, 100),
        );
    }

    fn draw_loading(&self, p: &mut Painter) {
        p.clear(rgb(6, 6, 8));
        p.text_centered(
            WIDTH as f32 / 2.0,
            230.0,
            "LOADING",
            FontStyle::bold(34.0),
            rgb(210, 210, 215),
        );
        let t = ((self.t.saturating_sub(MENU_UNTIL)) as f32 / (LOADING_UNTIL - MENU_UNTIL) as f32)
            .clamp(0.0, 1.0);
        hud::bar(
            p,
            Rect::new(240, 290, 480, 16),
            t,
            rgb(200, 200, 210),
            rgb(60, 60, 66),
        );
        p.text_centered(
            WIDTH as f32 / 2.0,
            330.0,
            "Tip: enemies come in packs.",
            FontStyle::regular(16.0),
            rgb(140, 140, 150),
        );
    }
}

fn wrap_angle(a: f32) -> f32 {
    let mut a = a;
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

/// A horned blob with glowing eyes.
fn enemy_sprite(size: u32, hurt: bool) -> RgbaImage {
    let mut img = RgbaImage::new(size, size);
    let s = size as f32;
    let mut p = Painter::new(&mut img);
    let body = if hurt {
        rgb(255, 200, 200)
    } else {
        rgb(150, 40, 70)
    };
    p.fill_polygon(
        &[
            (s * 0.22, s * 0.35),
            (s * 0.12, s * 0.08),
            (s * 0.36, s * 0.28),
        ],
        rgb(220, 210, 190),
    );
    p.fill_polygon(
        &[
            (s * 0.78, s * 0.35),
            (s * 0.88, s * 0.08),
            (s * 0.64, s * 0.28),
        ],
        rgb(220, 210, 190),
    );
    p.fill_ellipse(s * 0.5, s * 0.6, s * 0.36, s * 0.38, body);
    p.fill_circle(s * 0.38, s * 0.52, s * 0.08, rgb(255, 240, 120));
    p.fill_circle(s * 0.62, s * 0.52, s * 0.08, rgb(255, 240, 120));
    p.fill_polygon(
        &[
            (s * 0.36, s * 0.74),
            (s * 0.64, s * 0.74),
            (s * 0.5, s * 0.84),
        ],
        rgb(40, 10, 20),
    );
    img
}

impl Game for Dungeon {
    fn info(&self) -> SourceInfo {
        let mut info = SourceInfo::new(SourceKind::Window)
            .with_title("Dungeon 3D")
            .with_executable("dungeon3d.exe");
        info.executable_path = Some("C:\\Games\\Dungeon3D\\dungeon3d.exe".into());
        info
    }

    fn size(&self) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn update(&mut self, dt_ms: u64) {
        self.t += dt_ms;
        let dt = dt_ms as f32 / 1000.0;
        self.popups.retain(|p| p.1 > self.t);
        match self.phase {
            Phase::Menu if self.t >= MENU_UNTIL => self.set_phase(Phase::Loading),
            Phase::Loading if self.t >= LOADING_UNTIL => {
                self.play_started = self.t;
                self.next_spawn = self.t + 3000;
                self.ambushes = vec![25_000, 62_000, 100_000];
                self.set_phase(Phase::Play);
            }
            Phase::Play => self.play(dt),
            Phase::Dead { until } if self.t >= until => self.respawn(),
            Phase::Complete { until } if self.t >= until => {
                self.level += 1;
                self.respawn();
            }
            _ => {}
        }
    }

    fn render(&self) -> RgbaImage {
        let mut img = RgbaImage::new(WIDTH, HEIGHT);
        match self.phase {
            Phase::Menu => self.draw_menu(&mut Painter::new(&mut img)),
            Phase::Loading => self.draw_loading(&mut Painter::new(&mut img)),
            Phase::Play => {
                self.draw_world(&mut img);
                let mut p = Painter::new(&mut img);
                if self.t < self.hurt_until {
                    hud::veil(&mut p, rgb(200, 0, 0), 0.25);
                }
                if self.t < self.heal_until {
                    hud::veil(&mut p, rgb(40, 220, 90), 0.12);
                }
                self.draw_hud(&mut p);
            }
            Phase::Dead { .. } => {
                self.draw_world(&mut img);
                hud::desaturate(&mut img, 0.85);
                let mut p = Painter::new(&mut img);
                hud::veil(&mut p, rgb(120, 0, 0), 0.4);
                hud::banner(
                    &mut p,
                    WIDTH as f32 / 2.0,
                    190.0,
                    "YOU DIED",
                    72.0,
                    rgb(210, 20, 20),
                    rgb(20, 0, 0),
                );
                p.text_centered(
                    WIDTH as f32 / 2.0,
                    300.0,
                    "Press any key to continue",
                    FontStyle::bold(20.0),
                    rgb(220, 200, 200),
                );
            }
            Phase::Complete { .. } => {
                self.draw_world(&mut img);
                let mut p = Painter::new(&mut img);
                hud::veil(&mut p, rgb(0, 0, 0), 0.55);
                hud::banner(
                    &mut p,
                    WIDTH as f32 / 2.0,
                    180.0,
                    "LEVEL COMPLETE",
                    48.0,
                    rgb(250, 210, 90),
                    rgb(60, 40, 0),
                );
                p.text_centered(
                    WIDTH as f32 / 2.0,
                    260.0,
                    &format!("Gold collected: {}", self.gold),
                    FontStyle::bold(20.0),
                    rgb(230, 230, 230),
                );
            }
        }
        img
    }

    fn truth(&self) -> Truth {
        let mut elements = Vec::new();
        if self.phase == Phase::Play {
            let text_rect = |x: f32, y: f32, text: &str, size: f32| {
                let s = FontStyle::bold(size);
                Rect::new(
                    x as i32,
                    y as i32,
                    measure(text, s).ceil() as u32,
                    s.line_height().ceil() as u32,
                )
            };
            elements.push(TruthElement {
                name: "health".into(),
                concept: "health".into(),
                kind: "bar".into(),
                rect: HP_BAR,
                value: Some(self.health.ceil() as f64),
                max: Some(100.0),
                text: None,
                color: Some(HP_COLOR),
            });
            let ht = self.health_text();
            elements.push(TruthElement {
                name: "health_text".into(),
                concept: "health".into(),
                kind: "text".into(),
                rect: text_rect(270.0, 498.0, &ht, 18.0),
                value: Some(self.health.ceil() as f64),
                max: Some(100.0),
                text: Some(ht),
                color: None,
            });
            let ammo = self.ammo_text();
            let aw = measure(&ammo, FontStyle::bold(20.0));
            elements.push(TruthElement {
                name: "ammo".into(),
                concept: "ammo".into(),
                kind: "text".into(),
                rect: text_rect(940.0 - aw, 497.0, &ammo, 20.0),
                value: Some(self.ammo as f64),
                max: None,
                text: Some(ammo),
                color: None,
            });
            let gold = self.gold_text();
            elements.push(TruthElement {
                name: "gold".into(),
                concept: "currency".into(),
                kind: "text".into(),
                rect: text_rect(46.0, 16.0, &gold, 20.0),
                value: Some(self.gold as f64),
                max: None,
                text: Some(gold),
                color: None,
            });
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
    fn the_dungeon_plays_itself_and_dies_sometimes() {
        let mut d = Dungeon::new(3);
        let mut scenes = Vec::new();
        let mut died = 0;
        let mut defeated = 0;
        for _ in 0..(150_000 / 100) {
            d.update(100);
            for e in d.drain_events() {
                match e.kind {
                    TruthEventKind::SceneChanged { to } => scenes.push(to),
                    TruthEventKind::Died => died += 1,
                    TruthEventKind::EnemyDefeated => defeated += 1,
                    _ => {}
                }
            }
        }
        assert!(
            scenes.starts_with(&[SceneKind::Loading, SceneKind::Gameplay]),
            "{scenes:?}"
        );
        assert!(
            died >= 1,
            "the ambushes should kill the player at least once"
        );
        assert!(
            defeated >= 3,
            "the player should shoot enemies ({defeated})"
        );
        let img = d.render();
        assert_eq!(img.dimensions(), (WIDTH, HEIGHT));
    }
}
