//! Pictures and words for "what does Syrup see?": an observation drawn over
//! its frame, and a plain-text explanation.

use image::RgbaImage;
use syrup_core::{Observation, UiKind};
use syrup_paint::{Color, FontStyle, Painter, rgb, rgba};

fn kind_color(kind: &UiKind) -> Color {
    match kind {
        UiKind::Bar { .. } => rgb(255, 80, 80),
        UiKind::Minimap => rgb(80, 220, 255),
        UiKind::TextPanel => rgb(255, 210, 80),
        UiKind::Icon => rgb(200, 120, 255),
        UiKind::Panel => rgb(120, 255, 140),
        UiKind::Unknown => rgb(200, 200, 200),
    }
}

/// The frame with every region, text line and moving object outlined.
pub fn annotate(frame: &RgbaImage, obs: &Observation) -> RgbaImage {
    let mut img = frame.clone();
    let mut p = Painter::new(&mut img);
    let small = FontStyle::bold(11.0);
    for r in &obs.ui_regions {
        let c = kind_color(&r.kind);
        p.stroke_rect(r.rect.x, r.rect.y, r.rect.w as i32, r.rect.h as i32, 2, c);
        let label = match &r.kind {
            UiKind::Bar { fill, .. } => format!("#{} bar {:.0}%", r.id, fill * 100.0),
            k => format!("#{} {}", r.id, k.word()),
        };
        let y = if r.rect.y > 14 {
            r.rect.y as f32 - 13.0
        } else {
            r.rect.bottom() as f32 + 1.0
        };
        p.text_outlined(r.rect.x as f32, y, &label, small, c, rgb(0, 0, 0));
    }
    for t in &obs.text {
        p.stroke_rect(
            t.rect.x,
            t.rect.y,
            t.rect.w as i32,
            t.rect.h as i32,
            1,
            rgba(255, 255, 255, 200),
        );
    }
    for o in &obs.objects {
        p.stroke_rect(
            o.rect.x,
            o.rect.y,
            o.rect.w as i32,
            o.rect.h as i32,
            1,
            rgba(255, 0, 255, 220),
        );
    }
    let head = format!(
        "{} {:.0}%  {}",
        obs.scene.kind.word(),
        obs.scene.confidence.value() * 100.0,
        obs.scene.reason
    );
    p.fill_rect(
        0,
        0,
        (syrup_paint::measure(&head, small) + 12.0) as i32,
        17,
        rgba(0, 0, 0, 180),
    );
    p.text(6.0, 2.0, &head, small, rgb(255, 255, 255));
    img
}

/// A few lines saying what perception found.
pub fn explain(obs: &Observation) -> String {
    let mut out = format!(
        "frame {} at {:.1}s: {} ({}, {})\n",
        obs.frame_index,
        obs.timestamp_ms as f32 / 1000.0,
        obs.scene.kind.word(),
        obs.scene.confidence,
        obs.scene.reason
    );
    out.push_str(&format!(
        "  change {:.1}%  motion {:.1}%  brightness {:.2}  saturation {:.2}  analysis {:.1} ms\n",
        obs.metrics.change * 100.0,
        obs.metrics.motion * 100.0,
        obs.metrics.brightness,
        obs.metrics.saturation,
        obs.analysis_ms
    ));
    for r in &obs.ui_regions {
        let what = match &r.kind {
            UiKind::Bar { fill, color, .. } => {
                format!("bar, {:.0}% full, colour {:?}", fill * 100.0, color)
            }
            k => k.word().to_string(),
        };
        let labels: Vec<String> = obs
            .labels_of(r.id)
            .iter()
            .map(|t| format!("\"{}\"", t.text))
            .collect();
        out.push_str(&format!(
            "  region #{} at {} {:?}: {}{} (stability {:.2}, {})\n",
            r.id,
            r.norm.place(),
            (r.rect.x, r.rect.y, r.rect.w, r.rect.h),
            what,
            if labels.is_empty() {
                String::new()
            } else {
                format!(", text {}", labels.join(" "))
            },
            r.stability,
            r.confidence
        ));
    }
    let loose: Vec<String> = obs
        .text
        .iter()
        .filter(|t| t.region.is_none())
        .map(|t| format!("\"{}\"", t.text))
        .collect();
    if !loose.is_empty() {
        out.push_str(&format!("  text: {}\n", loose.join(", ")));
    }
    if !obs.objects.is_empty() {
        out.push_str(&format!("  {} moving things\n", obs.objects.len()));
    }
    for e in &obs.events {
        out.push_str(&format!(
            "  event: {}\n",
            serde_json::to_string(e).unwrap_or_default()
        ));
    }
    for u in &obs.uncertainties {
        out.push_str(&format!("  unsure about {}: {}\n", u.about, u.reason));
    }
    out
}
