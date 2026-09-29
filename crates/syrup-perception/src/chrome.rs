//! Desktop chrome in screen recordings: the taskbar under the game.
//!
//! A game window is captured without it, but a recording of the whole screen
//! (or `syrup live --screen`) also shows the operating system's taskbar, and
//! everything learned from such frames would be off: bars "at the bottom"
//! that are the taskbar's, positions that shift when the recording does not
//! include it. The taskbar gives itself away by what it always shows, the
//! clock and the date (or a search box), and it starts at a sharp edge
//! across the whole width.

use std::sync::OnceLock;

use image::RgbaImage;
use regex::Regex;
use syrup_core::Rect;

use crate::text::{OcrEngine, TextHint};

/// Whether text read from a strip of the screen is what a taskbar shows.
pub fn looks_like_taskbar(text: &str) -> bool {
    static CLOCK: OnceLock<Regex> = OnceLock::new();
    static DATE: OnceLock<Regex> = OnceLock::new();
    let clock = CLOCK.get_or_init(|| Regex::new(r"\b\d{1,2}[:.]\d{2}\b").expect("valid"));
    let date = DATE.get_or_init(|| Regex::new(r"\b\d{1,4}[/.-]\d{1,2}[/.-]\d{2,4}\b").expect("valid"));
    let lower = text.to_lowercase();
    // A full date at the bottom of the screen is the taskbar's; so is its search box.
    date.is_match(text) || lower.contains("type here to search") || (clock.is_match(text) && lower.contains("search"))
}

/// The row in `y0..y1` where the picture changes most sharply across (nearly)
/// the whole width, if any does.
pub fn full_width_edge(img: &RgbaImage, y0: u32, y1: u32) -> Option<u32> {
    let w = img.width();
    if w == 0 {
        return None;
    }
    let mut best: Option<(u32, f32)> = None;
    for y in y0.max(1)..y1.min(img.height()) {
        let strong = (0..w)
            .filter(|&x| {
                let (a, b) = (img.get_pixel(x, y).0, img.get_pixel(x, y - 1).0);
                (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).sum::<i32>() > 36
            })
            .count();
        let share = strong as f32 / w as f32;
        if share >= 0.6 && best.is_none_or(|(_, s)| share > s) {
            best = Some((y, share));
        }
    }
    best.map(|(y, _)| y)
}

/// The part of a screen recording that is the game, when a taskbar is along
/// the bottom; `None` when there is none (or no OCR to tell).
pub fn game_area(img: &RgbaImage, engine: &dyn OcrEngine) -> Option<Rect> {
    if !engine.is_available() {
        return None;
    }
    let (w, h) = img.dimensions();
    let band = ((h as f32) * 0.1).round() as u32;
    if band < 16 || w < 200 {
        return None;
    }
    let y0 = h - band;
    // Taskbar text is small: read it twice as big.
    let strip = image::imageops::crop_imm(img, 0, y0, w, band).to_image();
    let strip = image::imageops::resize(&strip, w * 2, band * 2, image::imageops::FilterType::CatmullRom);
    let lines = engine.read(&strip, TextHint::Sparse).ok()?;
    let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
    if !looks_like_taskbar(&text) {
        return None;
    }
    let edge = full_width_edge(img, y0, h.saturating_sub(4))?;
    Some(Rect::new(0, 0, w, edge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taskbar_reads_like_one() {
        assert!(looks_like_taskbar("Type here to search 7:22 PM 11/30/2025"));
        assert!(looks_like_taskbar("14:05 29.09.2026"));
        // What Tesseract really made of a Windows 10 taskbar.
        assert!(looks_like_taskbar("722M  Typ  92 e383 @ #  SE  @®.c’ eon  11/30/2025 B"));
        assert!(!looks_like_taskbar("71867 / 71867  29381 / 29906"));
        assert!(!looks_like_taskbar("Time Left 29:32"));
    }

    #[test]
    fn the_edge_is_found() {
        let mut img = RgbaImage::from_pixel(300, 200, image::Rgba([120, 60, 40, 255]));
        for y in 180..200 {
            for x in 0..300 {
                img.put_pixel(x, y, image::Rgba([30, 35, 45, 255]));
            }
        }
        assert_eq!(full_width_edge(&img, 170, 196), Some(180));
        let plain = RgbaImage::from_pixel(300, 200, image::Rgba([120, 60, 40, 255]));
        assert_eq!(full_width_edge(&plain, 170, 196), None);
    }
}
