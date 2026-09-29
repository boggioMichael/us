//! Syrup, drawn in layers.
//!
//! The dog is Maplesyrup's companion (its faces are cut from Maplesyrup's
//! animation by `tools/make_syrup_art.py`). On top of the face goes a hat
//! chosen for the game (the syrup cap for MapleStory and whenever nothing
//! better fits), always the **S** medallion under the chin, and a small prop
//! for the expression. Motion is rare and small: a blink now and then, a
//! gentle bob while speaking; with reduced motion, none.

use std::collections::HashMap;
use std::sync::OnceLock;

use image::RgbaImage;
use serde::Deserialize;
use syrup_core::{Expression, Hat};
use syrup_paint::{Painter, load_png, resize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Face {
    Talk,
    Laugh,
    Closed,
    Look,
}

impl Face {
    fn name(self) -> &'static str {
        match self {
            Face::Talk => "talk",
            Face::Laugh => "laugh",
            Face::Closed => "closed",
            Face::Look => "look",
        }
    }
}

#[derive(Deserialize)]
struct FaceMeta {
    size: [u32; 2],
    brim: [f32; 2],
}

#[derive(Deserialize)]
struct Meta {
    faces: HashMap<String, FaceMeta>,
    hat_anchor: [f32; 2],
}

struct Layers {
    faces: HashMap<Face, (RgbaImage, RgbaImage, (f32, f32))>,
    hats: HashMap<Hat, RgbaImage>,
    props: HashMap<&'static str, RgbaImage>,
    medallion: RgbaImage,
    anchor: (f32, f32),
}

macro_rules! asset {
    ($path:literal) => {
        include_bytes!(concat!("../../../assets/syrup/", $path)) as &[u8]
    };
}

fn layers() -> &'static Layers {
    static L: OnceLock<Layers> = OnceLock::new();
    L.get_or_init(|| {
        let meta: Meta =
            serde_json::from_str(include_str!("../../../assets/syrup/avatar.json")).expect("avatar.json is valid");
        let face_png: [(Face, &[u8], &[u8]); 4] = [
            (Face::Talk, asset!("base/face-talk.png"), asset!("base/face-talk-bare.png")),
            (Face::Laugh, asset!("base/face-laugh.png"), asset!("base/face-laugh-bare.png")),
            (Face::Closed, asset!("base/face-closed.png"), asset!("base/face-closed-bare.png")),
            (Face::Look, asset!("base/face-look.png"), asset!("base/face-look-bare.png")),
        ];
        let faces = face_png
            .iter()
            .map(|(f, capped, bare)| {
                let m = &meta.faces[f.name()];
                debug_assert_eq!(m.size[0], load_png(capped).width());
                (*f, (load_png(capped), load_png(bare), (m.brim[0], m.brim[1])))
            })
            .collect();
        let hat_png: [(Hat, &[u8]); 12] = [
            (Hat::WizardHat, asset!("hats/wizard_hat.png")),
            (Hat::KnightHelmet, asset!("hats/knight_helmet.png")),
            (Hat::RangerHood, asset!("hats/ranger_hood.png")),
            (Hat::RacingHelmet, asset!("hats/racing_helmet.png")),
            (Hat::TacticalHelmet, asset!("hats/tactical_helmet.png")),
            (Hat::AstronautHelmet, asset!("hats/astronaut_helmet.png")),
            (Hat::PirateHat, asset!("hats/pirate_hat.png")),
            (Hat::StrawHat, asset!("hats/straw_hat.png")),
            (Hat::DetectiveCap, asset!("hats/detective_cap.png")),
            (Hat::DealerVisor, asset!("hats/dealer_visor.png")),
            (Hat::SportsCap, asset!("hats/sports_cap.png")),
            (Hat::LanternHat, asset!("hats/lantern_hat.png")),
        ];
        let props: [(&'static str, &[u8]); 7] = [
            ("thinking", asset!("expressions/thinking.png")),
            ("researching", asset!("expressions/researching.png")),
            ("excited", asset!("expressions/excited.png")),
            ("proud", asset!("expressions/proud.png")),
            ("warning", asset!("expressions/warning.png")),
            ("confused", asset!("expressions/confused.png")),
            ("surprised", asset!("expressions/surprised.png")),
        ];
        Layers {
            faces,
            hats: hat_png.iter().map(|(h, b)| (*h, load_png(b))).collect(),
            props: props.iter().map(|(n, b)| (*n, load_png(b))).collect(),
            medallion: load_png(asset!("base/medallion.png")),
            anchor: (meta.hat_anchor[0], meta.hat_anchor[1]),
        }
    })
}

/// How Syrup looks at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub hat: Hat,
    pub expression: Expression,
    pub speaking: bool,
    /// Milliseconds on any clock, for the blink and the bob.
    pub t_ms: u64,
    pub reduced_motion: bool,
}

impl Pose {
    pub fn new(hat: Hat, expression: Expression) -> Self {
        Pose { hat, expression, speaking: false, t_ms: 0, reduced_motion: true }
    }
}

fn face_for(p: &Pose) -> Face {
    // A blink: eyes closed for a moment every few seconds.
    if !p.reduced_motion && p.t_ms % 5200 < 160 {
        return Face::Closed;
    }
    match p.expression {
        Expression::Neutral => Face::Talk,
        Expression::Thinking | Expression::Researching => Face::Closed,
        Expression::Excited | Expression::Proud => Face::Laugh,
        Expression::Warning | Expression::Confused | Expression::Surprised => Face::Look,
    }
}

fn prop_for(e: Expression) -> Option<&'static str> {
    match e {
        Expression::Neutral => None,
        Expression::Thinking => Some("thinking"),
        Expression::Researching => Some("researching"),
        Expression::Excited => Some("excited"),
        Expression::Proud => Some("proud"),
        Expression::Warning => Some("warning"),
        Expression::Confused => Some("confused"),
        Expression::Surprised => Some("surprised"),
    }
}

/// The full-size canvas: wide enough for any hat, tall enough for hat, face and medallion.
const CANVAS: (u32, u32) = (340, 470);
const FACE_AT: (i32, i32) = (31, 70);

/// Syrup, `height` pixels tall (the width follows: about 0.72 of the height).
pub fn render(pose: &Pose, height: u32) -> RgbaImage {
    let l = layers();
    let mut canvas = RgbaImage::new(CANVAS.0, CANVAS.1);
    let bob = if pose.speaking && !pose.reduced_motion {
        ((pose.t_ms as f32 / 1000.0 * std::f32::consts::TAU * 2.5).sin() * 5.0).round() as i32
    } else {
        0
    };
    let face = face_for(pose);
    let (capped, bare, brim) = &l.faces[&face];
    let (fx, fy) = (FACE_AT.0, FACE_AT.1 + bob);
    {
        let mut p = Painter::new(&mut canvas);
        let img = if pose.hat == Hat::SyrupCap { capped } else { bare };
        p.image(img, fx, fy, img.width(), img.height(), 1.0);
        if let Some(hat) = l.hats.get(&pose.hat) {
            let hx = fx + (brim.0 - l.anchor.0).round() as i32;
            let hy = fy + (brim.1 - l.anchor.1).round() as i32;
            p.image(hat, hx, hy, hat.width(), hat.height(), 1.0);
        }
        // The S medallion, always.
        let m = &l.medallion;
        let size = 76;
        p.image(m, CANVAS.0 as i32 / 2 - size / 2, fy + capped.height() as i32 - 60, size as u32, size as u32, 1.0);
        if let Some(name) = prop_for(pose.expression) {
            let pr = &l.props[name];
            let (px, py) = match name {
                "excited" => (8, 30),
                _ => (CANVAS.0 as i32 - 110, 24),
            };
            p.image(pr, px, py + bob / 2, 100, 100, 1.0);
        }
    }
    let width = (height as f32 * CANVAS.0 as f32 / CANVAS.1 as f32).round().max(1.0) as u32;
    resize(&canvas, width, height.max(1))
}

/// Every hat, side by side (for the docs and for choosing one).
pub fn hat_gallery(height: u32) -> RgbaImage {
    let hats = Hat::ALL;
    let one = render(&Pose::new(Hat::SyrupCap, Expression::Neutral), height);
    let (w, h) = one.dimensions();
    let cols = 7u32;
    let rows = (hats.len() as u32).div_ceil(cols);
    let mut sheet = RgbaImage::from_pixel(w * cols, (h + 24) * rows, image::Rgba([255, 244, 221, 255]));
    for (i, hat) in hats.iter().enumerate() {
        let img = render(&Pose::new(*hat, Expression::Neutral), height);
        let (x, y) = ((i as u32 % cols) * w, (i as u32 / cols) * (h + 24));
        let mut p = Painter::new(&mut sheet);
        p.image(&img, x as i32, y as i32, w, h, 1.0);
        p.text_centered(
            x as f32 + w as f32 / 2.0,
            (y + h + 2) as f32,
            &hat.name().replace('_', " "),
            syrup_paint::FontStyle::bold(14.0),
            syrup_paint::rgb(0x57, 0x35, 0x1F),
        );
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(img: &RgbaImage, x: u32, y: u32) -> bool {
        img.get_pixel(x, y).0[3] > 200
    }

    #[test]
    fn every_hat_renders_with_the_medallion() {
        for hat in Hat::ALL {
            for e in Expression::ALL {
                let img = render(&Pose::new(hat, e), 235);
                assert_eq!(img.height(), 235);
                // The medallion: gold, under the chin, in the middle.
                let (w, h) = img.dimensions();
                let mid = (w / 2, (h as f32 * 0.83) as u32);
                let p = img.get_pixel(mid.0, mid.1).0;
                assert!(opaque(&img, mid.0, mid.1), "{hat:?} {e:?}: nothing at the medallion");
                let gold_or_ink = (p[0] > 150 && p[1] > 100 && p[2] < 120) || (p[0] < 120 && p[1] < 80);
                assert!(gold_or_ink, "{hat:?} {e:?}: medallion pixel {p:?}");
            }
        }
    }

    #[test]
    fn hats_cover_the_cap_and_change_the_look() {
        let cap = render(&Pose::new(Hat::SyrupCap, Expression::Neutral), 300);
        let wizard = render(&Pose::new(Hat::WizardHat, Expression::Neutral), 300);
        let knight = render(&Pose::new(Hat::KnightHelmet, Expression::Neutral), 300);
        let top = |img: &RgbaImage| img.enumerate_pixels().filter(|(_, y, p)| *y < 110 && p.0[3] > 128).count();
        assert!(top(&wizard) > 0 && top(&knight) > 0 && top(&cap) > 0);
        assert_ne!(wizard.as_raw(), knight.as_raw());
        // The wizard hat is blue up top; the syrup cap is honey and white.
        let blue = |img: &RgbaImage| {
            img.enumerate_pixels()
                .filter(|(_, y, p)| *y < 110 && p.0[3] > 200 && p.0[2] as i32 > p.0[0] as i32 + 40)
                .count()
        };
        assert!(blue(&wizard) > 500 && blue(&cap) < 50, "{} {}", blue(&wizard), blue(&cap));
    }

    #[test]
    fn motion_only_when_allowed() {
        let mut p = Pose::new(Hat::SyrupCap, Expression::Neutral);
        p.speaking = true;
        p.t_ms = 100;
        let still = render(&p, 200);
        p.t_ms = 250;
        assert_eq!(still.as_raw(), render(&p, 200).as_raw(), "reduced motion: no bob");
        p.reduced_motion = false;
        assert_ne!(still.as_raw(), render(&p, 200).as_raw(), "speaking bobs");
    }
}
