//! A source with every frame cut to one rectangle: a screen recording
//! without its taskbar, say.

use std::sync::Arc;

use syrup_core::{Frame, Rect, SourceInfo};

use crate::{Capture, CaptureError, FrameSource};

pub struct Cropped<S> {
    inner: S,
    rect: Rect,
    first: Option<Frame>,
}

impl<S: FrameSource> Cropped<S> {
    pub fn new(inner: S, rect: Rect) -> Self {
        Cropped { inner, rect, first: None }
    }

    /// Starts with a frame already taken from `inner` (to look at it before deciding).
    pub fn with_first(inner: S, rect: Rect, first: Frame) -> Self {
        Cropped { inner, rect, first: Some(first) }
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }
}

/// `frame` cut to `rect` (clipped to the frame); its origin moves with it.
pub fn crop_frame(frame: Frame, rect: Rect) -> Frame {
    let Some(r) = rect.clip(frame.width(), frame.height()) else { return frame };
    if r.x == 0 && r.y == 0 && r.w == frame.width() && r.h == frame.height() {
        return frame;
    }
    let image = image::imageops::crop_imm(&*frame.image, r.x as u32, r.y as u32, r.w, r.h).to_image();
    Frame { image: Arc::new(image), origin: (frame.origin.0 + r.x, frame.origin.1 + r.y), ..frame }
}

impl<S: FrameSource> FrameSource for Cropped<S> {
    fn info(&self) -> Arc<SourceInfo> {
        self.inner.info()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        if let Some(f) = self.first.take() {
            return Ok(Capture::Frame(crop_frame(f, self.rect)));
        }
        Ok(match self.inner.next()? {
            Capture::Frame(f) => Capture::Frame(crop_frame(f, self.rect)),
            other => other,
        })
    }

    fn nominal_fps(&self) -> f32 {
        self.inner.nominal_fps()
    }

    fn len_hint(&self) -> Option<u64> {
        self.inner.len_hint()
    }

    fn is_live(&self) -> bool {
        self.inner.is_live()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemorySource;
    use image::RgbaImage;

    #[test]
    fn frames_are_cut_and_keep_their_place() {
        let frames = vec![RgbaImage::new(100, 80), RgbaImage::new(100, 80)];
        let mut src = Cropped::new(MemorySource::at_fps(frames, 2.0), Rect::new(0, 0, 100, 60));
        let Capture::Frame(f) = src.next().unwrap() else { panic!() };
        assert_eq!(f.size(), (100, 60));
        let Capture::Frame(f) = src.next().unwrap() else { panic!() };
        assert_eq!((f.size(), f.origin), ((100, 60), (0, 0)));
        assert!(matches!(src.next().unwrap(), Capture::Ended));
    }
}
