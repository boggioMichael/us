//! Deciding which frames to analyse.
//!
//! Capture never waits for analysis. Every frame gets a cheap change probe
//! (a 64x36 luminance thumbnail); full analysis runs about twice a second
//! on a quiet screen and up to ten times a second on a busy one, right away
//! after a scene cut, and less often when the last analysis ran over its
//! budget.

use syrup_core::Frame;

use crate::pixels::{thumb_difference, thumbnail};

#[derive(Debug, Clone)]
pub struct SamplerConfig {
    pub min_interval_ms: u64,
    pub max_interval_ms: u64,
    /// An analysis slower than this makes the sampler back off.
    pub budget_ms: f32,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        SamplerConfig { min_interval_ms: 100, max_interval_ms: 500, budget_ms: 60.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleDecision {
    pub analyse: bool,
    /// Change since the last analysed frame, 0..1.
    pub change: f32,
    /// How busy the screen is, 0..1.
    pub busy: f32,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct FrameSampler {
    pub cfg: SamplerConfig,
    prev: Vec<u8>,
    analysed: Vec<u8>,
    last_ms: Option<u64>,
    busy: f32,
    last_cost_ms: f32,
}

impl FrameSampler {
    pub fn new(cfg: SamplerConfig) -> Self {
        FrameSampler { cfg, ..Default::default() }
    }

    pub fn decide(&mut self, frame: &Frame) -> SampleDecision {
        let thumb = thumbnail(&frame.image, 64, 36);
        let step = if self.prev.is_empty() { 0.0 } else { thumb_difference(&thumb, &self.prev) };
        self.busy = self.busy * 0.7 + (step * 25.0).min(1.0) * 0.3;
        let change = if self.analysed.is_empty() { 1.0 } else { thumb_difference(&thumb, &self.analysed) };
        let span = (self.cfg.max_interval_ms - self.cfg.min_interval_ms.min(self.cfg.max_interval_ms)) as f32;
        let mut interval = self.cfg.max_interval_ms as f32 - span * self.busy;
        if self.last_cost_ms > self.cfg.budget_ms {
            interval = interval.max(self.last_cost_ms * 3.0);
        }
        let interval = interval as u64;
        let now = frame.timestamp_ms;
        let elapsed = self.last_ms.map(|t| now.saturating_sub(t));
        let analyse = match elapsed {
            None => true,
            Some(e) => e >= interval || (change > 0.2 && e >= self.cfg.min_interval_ms),
        };
        self.prev = thumb;
        if analyse {
            self.analysed = self.prev.clone();
            self.last_ms = Some(now);
        }
        SampleDecision { analyse, change, busy: self.busy, interval_ms: interval }
    }

    /// How long the analysis of the last sampled frame took.
    pub fn record_cost(&mut self, ms: f32) {
        self.last_cost_ms = ms;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::RgbaImage;
    use syrup_core::frame::{SourceInfo, SourceKind};

    use super::*;

    fn frame(t: u64, v: u8) -> Frame {
        Frame::new(
            t / 50,
            t,
            RgbaImage::from_pixel(64, 36, image::Rgba([v, v, v, 255])),
            Arc::new(SourceInfo::new(SourceKind::Images)),
        )
    }

    #[test]
    fn quiet_screens_are_analysed_less_often_and_cuts_right_away() {
        let mut s = FrameSampler::new(SamplerConfig::default());
        let mut analysed = 0;
        for i in 0..40u64 {
            analysed += s.decide(&frame(i * 50, 100)).analyse as u32;
        }
        // Two seconds of a still screen: about four analyses.
        assert!((4..=6).contains(&analysed), "{analysed}");
        // A cut is analysed at once.
        let d = s.decide(&frame(40 * 50, 250));
        assert!(d.analyse && d.change > 0.5);
        s.record_cost(400.0);
        assert!(s.decide(&frame(41 * 50, 10)).interval_ms >= 1200);
    }
}
