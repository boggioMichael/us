//! What the player sees and hears: Syrup's overlay and voice.
//!
//! The overlay is painted in software ([`paint`]) so it looks the same
//! everywhere and can be tested: Syrup with the game's hat, a card with the
//! line and the evidence, and one-click feedback. On Windows ([`win`]) the
//! picture goes into a layered window over the game: see-through, clicks
//! pass to the game everywhere except on the card, it never takes the focus,
//! and it is left out of screen captures so Syrup never reads its own card.
//! Elsewhere, or headless, the same picture can be saved or shown in the
//! devtools page.
//!
//! Colours come from Maplesyrup's companion: ink `#57351F`, cream `#FFF4DD`,
//! amber `#D99A43`, honey `#FFE5B8`.

#[cfg(windows)]
pub mod win;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use syrup_avatar::{Pose, render};
use syrup_core::{Expression, FeedbackKind, Hat, Rect};
use syrup_paint::{Color, FontStyle, Painter, measure, rgb, rgba, wrap};

pub const INK: Color = rgb(0x57, 0x35, 0x1F);
pub const CREAM: Color = rgb(0xFF, 0xF4, 0xDD);
pub const AMBER: Color = rgb(0xD9, 0x9A, 0x43);
pub const HONEY: Color = rgb(0xFF, 0xE5, 0xB8);
pub const LINE: Color = rgb(0xE7, 0xC9, 0x9C);
pub const MUTED: Color = rgb(0x9A, 0x7B, 0x5E);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    Hidden,
    /// Syrup's head and one short line.
    Minimal,
    /// The card: the line, why, and feedback.
    #[default]
    Normal,
    /// Normal, plus what perception and the state engine see.
    Analysis,
    /// The session's summary.
    PostGame,
}

impl OverlayMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "hidden" | "off" => Some(OverlayMode::Hidden),
            "minimal" => Some(OverlayMode::Minimal),
            "normal" => Some(OverlayMode::Normal),
            "analysis" => Some(OverlayMode::Analysis),
            "post_game" | "postgame" | "summary" => Some(OverlayMode::PostGame),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            OverlayMode::Hidden => OverlayMode::Minimal,
            OverlayMode::Minimal => OverlayMode::Normal,
            OverlayMode::Normal => OverlayMode::Analysis,
            OverlayMode::Analysis | OverlayMode::PostGame => OverlayMode::Hidden,
        }
    }
}

/// The line Syrup is saying.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub advice_id: u64,
    pub topic: String,
    pub text: String,
    /// "wiki · 76%", "I saw it · 90%".
    pub source: String,
}

/// Everything the overlay shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayView {
    pub mode: OverlayMode,
    pub hat: Hat,
    pub expression: Expression,
    pub speaking: bool,
    pub line: Option<Line>,
    /// What Syrup is doing when it has nothing to say: "Watching. Learning this game."
    pub status: String,
    /// Analysis mode: short lines (game, scene, concepts, speed).
    pub analysis: Vec<String>,
    /// Post-game mode: the summary.
    pub summary: Vec<String>,
    pub reduced_motion: bool,
    /// UI scale (1 at 96 dpi).
    pub scale: f32,
}

impl Default for OverlayView {
    fn default() -> Self {
        OverlayView {
            mode: OverlayMode::Normal,
            hat: Hat::SyrupCap,
            expression: Expression::Neutral,
            speaking: false,
            line: None,
            status: "Watching.".into(),
            analysis: Vec::new(),
            summary: Vec::new(),
            reduced_motion: false,
            scale: 1.0,
        }
    }
}

/// What the player clicked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UiAction {
    Feedback { advice_id: u64, topic: String, kind: FeedbackKind },
    NextMode,
}

/// A clickable part of the painted overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub action: UiAction,
    pub label: &'static str,
}

/// The painted overlay: its picture and where its buttons are.
#[derive(Debug, Clone)]
pub struct Painted {
    pub image: RgbaImage,
    pub buttons: Vec<Button>,
}

impl Painted {
    pub fn action_at(&self, x: i32, y: i32) -> Option<UiAction> {
        self.buttons.iter().find(|b| b.rect.contains(x, y)).map(|b| b.action.clone())
    }
}

fn icon(p: &mut Painter, kind: FeedbackKind, cx: f32, cy: f32, s: f32) {
    match kind {
        FeedbackKind::Useful => {
            p.line(cx - 5.0 * s, cy, cx - 1.5 * s, cy + 4.0 * s, 2.4 * s, rgb(60, 140, 70));
            p.line(cx - 1.5 * s, cy + 4.0 * s, cx + 6.0 * s, cy - 5.0 * s, 2.4 * s, rgb(60, 140, 70));
        }
        FeedbackKind::Wrong => {
            p.line(cx - 5.0 * s, cy - 5.0 * s, cx + 5.0 * s, cy + 5.0 * s, 2.4 * s, rgb(190, 60, 50));
            p.line(cx - 5.0 * s, cy + 5.0 * s, cx + 5.0 * s, cy - 5.0 * s, 2.4 * s, rgb(190, 60, 50));
        }
        FeedbackKind::Explain => {
            let st = FontStyle::bold(15.0 * s);
            p.text_centered(cx, cy - st.line_height() / 2.0, "?", st, INK);
        }
        FeedbackKind::StopSuggesting => {
            // A speaker, crossed out.
            p.fill_polygon(
                &[
                    (cx - 6.0 * s, cy - 2.5 * s),
                    (cx - 3.0 * s, cy - 2.5 * s),
                    (cx + 1.5 * s, cy - 6.0 * s),
                    (cx + 1.5 * s, cy + 6.0 * s),
                    (cx - 3.0 * s, cy + 2.5 * s),
                    (cx - 6.0 * s, cy + 2.5 * s),
                ],
                INK,
            );
            p.line(cx + 3.5 * s, cy - 4.0 * s, cx + 7.5 * s, cy + 4.0 * s, 1.8 * s, rgb(190, 60, 50));
            p.line(cx + 7.5 * s, cy - 4.0 * s, cx + 3.5 * s, cy + 4.0 * s, 1.8 * s, rgb(190, 60, 50));
        }
        FeedbackKind::Research => {
            p.stroke_circle(cx - 1.5 * s, cy - 1.5 * s, 4.5 * s, 2.0 * s, INK);
            p.line(cx + 2.0 * s, cy + 2.0 * s, cx + 6.0 * s, cy + 6.0 * s, 2.2 * s, INK);
        }
        FeedbackKind::Ignore => {}
    }
}

/// Paints the overlay for `view` at time `t_ms`.
pub fn paint(view: &OverlayView, t_ms: u64) -> Painted {
    let s = view.scale.clamp(0.5, 3.0);
    let pose = Pose {
        hat: view.hat,
        expression: view.expression,
        speaking: view.speaking,
        t_ms,
        reduced_motion: view.reduced_motion,
    };
    match view.mode {
        OverlayMode::Hidden => Painted { image: RgbaImage::new(1, 1), buttons: Vec::new() },
        OverlayMode::Minimal => {
            let head_h = (84.0 * s) as u32;
            let head = render(&pose, head_h);
            let text = view.line.as_ref().map(|l| l.text.clone()).unwrap_or_else(|| view.status.clone());
            let st = FontStyle::bold(14.0 * s);
            let lines = wrap(&text, st, 260.0 * s);
            let line = lines.first().cloned().unwrap_or_default();
            let line = if lines.len() > 1 { format!("{line}…") } else { line };
            let tw = measure(&line, st);
            let (w, h) = ((head.width() as f32 + tw + 34.0 * s) as u32, head_h);
            let mut img = RgbaImage::new(w, h);
            let mut p = Painter::new(&mut img);
            let pill_h = st.line_height() + 14.0 * s;
            let px = head.width() as f32 - 6.0 * s;
            let py = h as f32 / 2.0 - pill_h / 2.0;
            p.fill_rounded_rect(px, py, tw + 28.0 * s, pill_h, pill_h / 2.0, with_alpha(CREAM, 0.96));
            p.stroke_rounded_rect(px, py, tw + 28.0 * s, pill_h, pill_h / 2.0, 1.3 * s, INK);
            p.text(px + 16.0 * s, py + 7.0 * s, &line, st, INK);
            p.image(&head, 0, 0, head.width(), head.height(), 1.0);
            Painted {
                image: img,
                buttons: vec![Button {
                    rect: Rect::new(0, 0, head.width(), head.height()),
                    action: UiAction::NextMode,
                    label: "mode",
                }],
            }
        }
        OverlayMode::Normal | OverlayMode::Analysis | OverlayMode::PostGame => paint_card(view, &pose, s),
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    syrup_paint::with_alpha(c, a)
}

fn paint_card(view: &OverlayView, pose: &Pose, s: f32) -> Painted {
    let card_w = 380.0 * s;
    let head_h = (104.0 * s) as u32;
    let head = render(pose, head_h);
    let pad = 12.0 * s;
    let text_x = head.width() as f32 + 8.0 * s;
    let text_w = card_w - text_x - pad;
    let big = FontStyle::bold(15.0 * s);
    let small = FontStyle::regular(11.0 * s);
    let (title, body, source): (Option<String>, Vec<String>, Option<String>) = match view.mode {
        OverlayMode::PostGame => (
            Some("How it went".into()),
            view.summary.iter().flat_map(|l| wrap(l, FontStyle::regular(13.0 * s), text_w)).collect(),
            None,
        ),
        _ => match &view.line {
            Some(l) => (None, wrap(&l.text, big, text_w), Some(l.source.clone())),
            None => (None, wrap(&view.status, FontStyle::regular(13.0 * s), text_w), None),
        },
    };
    let body_style =
        if view.line.is_some() && view.mode != OverlayMode::PostGame { big } else { FontStyle::regular(13.0 * s) };
    let mut text_h = body.len() as f32 * body_style.line_height();
    if title.is_some() {
        text_h += big.line_height() + 4.0 * s;
    }
    if source.is_some() {
        text_h += small.line_height() + 4.0 * s;
    }
    let buttons_h = if view.line.is_some() && view.mode != OverlayMode::PostGame { 30.0 * s } else { 0.0 };
    let analysis: Vec<String> =
        if view.mode == OverlayMode::Analysis { view.analysis.iter().take(14).cloned().collect() } else { Vec::new() };
    let mono = FontStyle::regular(11.0 * s);
    let analysis_h = if analysis.is_empty() { 0.0 } else { analysis.len() as f32 * mono.line_height() + 16.0 * s };
    let top_h = (head_h as f32).max(text_h + pad * 2.0);
    let card_h = top_h + buttons_h + analysis_h + 6.0 * s;
    let (w, h) = (card_w.ceil() as u32 + 4, card_h.ceil() as u32 + 4);
    let mut img = RgbaImage::new(w, h);
    let mut buttons = Vec::new();
    {
        let mut p = Painter::new(&mut img);
        // Card with a soft shadow.
        p.fill_rounded_rect(3.0, 4.0, card_w - 2.0, card_h - 2.0, 12.0 * s, rgba(60, 35, 20, 60));
        p.fill_rounded_rect(1.0, 1.0, card_w - 2.0, card_h - 2.0, 12.0 * s, with_alpha(CREAM, 0.97));
        p.stroke_rounded_rect(1.0, 1.0, card_w - 2.0, card_h - 2.0, 12.0 * s, 1.4 * s, AMBER);
        p.image(
            &head,
            (2.0 * s) as i32,
            ((top_h - head_h as f32) / 2.0).max(0.0) as i32,
            head.width(),
            head.height(),
            1.0,
        );
        let mut y = pad;
        if let Some(t) = &title {
            p.text(text_x, y, t, big, INK);
            y += big.line_height() + 4.0 * s;
        }
        let color = if view.line.is_some() || view.mode == OverlayMode::PostGame { INK } else { MUTED };
        for l in &body {
            p.text(text_x, y, l, body_style, color);
            y += body_style.line_height();
        }
        if let Some(src) = &source {
            y += 4.0 * s;
            p.text(text_x, y, src, small, MUTED);
        }
        // Feedback: 👍 👎 ❓ 🔇, then Research and Ignore.
        if let (Some(l), true) = (&view.line, buttons_h > 0.0) {
            let by = top_h - 2.0 * s;
            let mut bx = text_x;
            for (kind, label) in [
                (FeedbackKind::Useful, "useful"),
                (FeedbackKind::Wrong, "wrong"),
                (FeedbackKind::Explain, "why?"),
                (FeedbackKind::StopSuggesting, "stop"),
            ] {
                let bw = 30.0 * s;
                p.fill_rounded_rect(bx, by, bw, 24.0 * s, 7.0 * s, HONEY);
                p.stroke_rounded_rect(bx, by, bw, 24.0 * s, 7.0 * s, 1.0 * s, LINE);
                icon(&mut p, kind, bx + bw / 2.0, by + 12.0 * s, s);
                buttons.push(Button {
                    rect: Rect::new(bx as i32, by as i32, bw as u32, (24.0 * s) as u32),
                    action: UiAction::Feedback { advice_id: l.advice_id, topic: l.topic.clone(), kind },
                    label,
                });
                bx += bw + 6.0 * s;
            }
            for (kind, label) in [(FeedbackKind::Research, "research"), (FeedbackKind::Ignore, "ignore")] {
                let st = FontStyle::bold(11.0 * s);
                let name = if kind == FeedbackKind::Research { "Research" } else { "Ignore" };
                let bw = measure(name, st) + 16.0 * s;
                p.fill_rounded_rect(
                    bx,
                    by,
                    bw,
                    24.0 * s,
                    7.0 * s,
                    if kind == FeedbackKind::Research { HONEY } else { with_alpha(CREAM, 1.0) },
                );
                p.stroke_rounded_rect(bx, by, bw, 24.0 * s, 7.0 * s, 1.0 * s, LINE);
                p.text(bx + 8.0 * s, by + (24.0 * s - st.line_height()) / 2.0, name, st, INK);
                buttons.push(Button {
                    rect: Rect::new(bx as i32, by as i32, bw as u32, (24.0 * s) as u32),
                    action: UiAction::Feedback { advice_id: l.advice_id, topic: l.topic.clone(), kind },
                    label,
                });
                bx += bw + 6.0 * s;
            }
        }
        if !analysis.is_empty() {
            let ay = top_h + buttons_h + 4.0 * s;
            p.fill_rect(pad as i32, ay as i32, (card_w - 2.0 * pad) as i32, (1.0 * s).max(1.0) as i32, LINE);
            let mut y = ay + 8.0 * s;
            for l in &analysis {
                p.text(pad, y, l, mono, INK);
                y += mono.line_height();
            }
        }
    }
    buttons.push(Button {
        rect: Rect::new(0, 0, head.width(), head.height()),
        action: UiAction::NextMode,
        label: "mode",
    });
    Painted { image: img, buttons }
}

/// Speaks Syrup's lines (the system voice on Windows; silent elsewhere).
pub struct Voice {
    #[cfg(windows)]
    inner: Option<win::Speech>,
    pub enabled: bool,
}

impl Voice {
    pub fn new(enabled: bool) -> Self {
        Voice {
            #[cfg(windows)]
            inner: if enabled { win::Speech::start(1) } else { None },
            enabled,
        }
    }

    /// Says `line`, cutting off whatever was being said.
    pub fn say(&self, line: &str) {
        if !self.enabled {
            return;
        }
        #[cfg(windows)]
        if let Some(v) = &self.inner {
            v.say(line);
        }
        #[cfg(not(windows))]
        let _ = line;
    }

    pub fn available(&self) -> bool {
        #[cfg(windows)]
        {
            self.inner.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(mode: OverlayMode) -> OverlayView {
        OverlayView {
            mode,
            line: Some(Line {
                advice_id: 7,
                topic: "low:health".into(),
                text: "Careful. Health at 20%.".into(),
                source: "I saw it · 90%".into(),
            }),
            analysis: vec!["Sky Meadow Online (unknown, 40%)".into(), "health 0.20 (88%)".into()],
            summary: vec!["12 min, 2 deaths.".into()],
            ..Default::default()
        }
    }

    #[test]
    fn the_card_has_its_buttons_where_they_are_drawn() {
        let painted = paint(&view(OverlayMode::Normal), 0);
        let feedback: Vec<&Button> =
            painted.buttons.iter().filter(|b| matches!(b.action, UiAction::Feedback { .. })).collect();
        assert_eq!(feedback.len(), 6);
        for b in feedback {
            let (cx, cy) = b.rect.center();
            assert!(painted.image.get_pixel(cx as u32, cy as u32).0[3] > 200, "button {} is drawn", b.label);
            assert!(matches!(painted.action_at(cx as i32, cy as i32), Some(UiAction::Feedback { advice_id: 7, .. })));
        }
        // Outside the card: see-through (clicks go to the game).
        let (w, h) = painted.image.dimensions();
        assert_eq!(painted.image.get_pixel(w - 1, h - 1).0[3], 0);
    }

    #[test]
    fn every_mode_paints() {
        let normal = paint(&view(OverlayMode::Normal), 0).image;
        let analysis = paint(&view(OverlayMode::Analysis), 0).image;
        assert!(analysis.height() > normal.height());
        let minimal = paint(&view(OverlayMode::Minimal), 0).image;
        assert!(minimal.height() < normal.height());
        assert!(paint(&view(OverlayMode::PostGame), 0).image.width() > 100);
        assert_eq!(paint(&view(OverlayMode::Hidden), 0).image.dimensions(), (1, 1));
        let big = paint(&OverlayView { scale: 2.0, ..view(OverlayMode::Normal) }, 0).image;
        assert!(big.width() > normal.width() * 3 / 2);
    }
}
