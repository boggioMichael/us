//! Frames already in memory: for tests and tools.

use std::collections::VecDeque;
use std::sync::Arc;

use image::RgbaImage;
use syrup_core::Frame;
use syrup_core::frame::{SourceInfo, SourceKind};

use crate::{Capture, CaptureError, FrameSource};

pub struct MemorySource {
    frames: VecDeque<(u64, RgbaImage)>,
    index: u64,
    fps: f32,
    info: Arc<SourceInfo>,
}

impl MemorySource {
    /// Frames with their timestamps (ms).
    pub fn new(frames: Vec<(u64, RgbaImage)>, info: SourceInfo) -> Self {
        let fps = match (frames.first(), frames.last()) {
            (Some(a), Some(b)) if frames.len() > 1 && b.0 > a.0 => {
                (frames.len() - 1) as f32 * 1000.0 / (b.0 - a.0) as f32
            }
            _ => 10.0,
        };
        MemorySource { frames: frames.into(), index: 0, fps, info: Arc::new(info) }
    }

    /// Evenly spaced frames.
    pub fn at_fps(images: Vec<RgbaImage>, fps: f32) -> Self {
        let step = 1000.0 / fps.max(0.01);
        let frames = images.into_iter().enumerate().map(|(i, img)| ((i as f32 * step).round() as u64, img)).collect();
        let mut s = MemorySource::new(frames, SourceInfo::new(SourceKind::Images));
        s.fps = fps;
        s
    }
}

impl FrameSource for MemorySource {
    fn info(&self) -> Arc<SourceInfo> {
        self.info.clone()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        match self.frames.pop_front() {
            Some((ts, image)) => {
                let f = Frame::new(self.index, ts, image, self.info.clone());
                self.index += 1;
                Ok(Capture::Frame(f))
            }
            None => Ok(Capture::Ended),
        }
    }

    fn nominal_fps(&self) -> f32 {
        self.fps
    }

    fn len_hint(&self) -> Option<u64> {
        Some(self.index + self.frames.len() as u64)
    }
}
