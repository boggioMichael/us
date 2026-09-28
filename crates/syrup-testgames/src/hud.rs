//! HUD pieces the test games share: framed bars, labels with shadows, panels.

use syrup_core::Rect;
use syrup_paint::{Color, FontStyle, Painter, lerp_color, rgb, rgba};

/// A horizontal bar: dark track, `fill` (0..1) of `color` with a highlight, a frame.
pub fn bar(p: &mut Painter, r: Rect, fill: f32, color: Color, frame: Color) {
    let fill = fill.clamp(0.0, 1.0);
    p.fill_rect(r.x, r.y, r.w as i32, r.h as i32, frame);
    let inner = Rect::new(
        r.x + 2,
        r.y + 2,
        r.w.saturating_sub(4),
        r.h.saturating_sub(4),
    );
    p.fill_rect(
        inner.x,
        inner.y,
        inner.w as i32,
        inner.h as i32,
        rgb(28, 24, 30),
    );
    let fw = (inner.w as f32 * fill).round() as i32;
    if fw > 0 {
        let light = lerp_color(color, rgb(255, 255, 255), 0.35);
        let dark = lerp_color(color, rgb(0, 0, 0), 0.25);
        let h = inner.h as i32;
        let top = (h / 3).max(1);
        p.gradient_rect(inner.x, inner.y, fw, top, light, color);
        p.gradient_rect(inner.x, inner.y + top, fw, h - top, color, dark);
    }
}

/// Text with a one-pixel dark shadow, top-left at `(x, y)`; returns its width.
pub fn label(p: &mut Painter, x: f32, y: f32, text: &str, size: f32, color: Color) -> f32 {
    let style = FontStyle::bold(size);
    p.text(x + 1.5, y + 1.5, text, style, rgba(0, 0, 0, 200));
    p.text(x, y, text, style, color)
}

/// Centred text with an outline, for the big screens ("YOU DIED").
pub fn banner(
    p: &mut Painter,
    cx: f32,
    y: f32,
    text: &str,
    size: f32,
    color: Color,
    outline: Color,
) {
    let style = FontStyle::bold(size);
    let w = syrup_paint::measure(text, style);
    p.text_outlined(cx - w / 2.0, y, text, style, color, outline);
}

/// A rounded panel with a border.
pub fn panel(p: &mut Painter, r: Rect, fill: Color, border: Color) {
    p.fill_rounded_rect(r.x as f32, r.y as f32, r.w as f32, r.h as f32, 8.0, fill);
    p.stroke_rounded_rect(
        r.x as f32, r.y as f32, r.w as f32, r.h as f32, 8.0, 2.0, border,
    );
}

/// A menu button, highlighted or not.
pub fn button(p: &mut Painter, r: Rect, text: &str, size: f32, highlighted: bool) {
    let (fill, border, ink) = if highlighted {
        (rgb(250, 210, 90), rgb(255, 245, 200), rgb(40, 30, 10))
    } else {
        (
            rgba(20, 20, 30, 220),
            rgb(160, 160, 180),
            rgb(230, 230, 240),
        )
    };
    p.fill_rounded_rect(r.x as f32, r.y as f32, r.w as f32, r.h as f32, 6.0, fill);
    p.stroke_rounded_rect(
        r.x as f32, r.y as f32, r.w as f32, r.h as f32, 6.0, 2.0, border,
    );
    let style = FontStyle::bold(size);
    p.text_centered(
        r.x as f32 + r.w as f32 / 2.0,
        r.y as f32 + (r.h as f32 - style.line_height()) / 2.0 + 1.0,
        text,
        style,
        ink,
    );
}

/// Darkens (or tints) the whole picture: `a` of `color` over everything.
pub fn veil(p: &mut Painter, color: Color, a: f32) {
    let (w, h) = (p.width() as i32, p.height() as i32);
    let c = [
        color[0],
        color[1],
        color[2],
        (a.clamp(0.0, 1.0) * 255.0) as u8,
    ];
    p.fill_rect(0, 0, w, h, c);
}

/// Drains colour out of the picture (towards grey), by `amount`.
pub fn desaturate(img: &mut image::RgbaImage, amount: f32) {
    for px in img.pixels_mut() {
        let [r, g, b, a] = px.0;
        let l = 0.3 * r as f32 + 0.59 * g as f32 + 0.11 * b as f32;
        let mix = |v: u8| {
            (v as f32 + (l - v as f32) * amount)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        px.0 = [mix(r), mix(g), mix(b), a];
    }
}
