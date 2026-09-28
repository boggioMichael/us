//! Where frames come from.
//!
//! Every source hands out [`Frame`]s through one trait, [`FrameSource`], so the
//! rest of Syrup cannot tell a live game window from a recorded video, a
//! folder of screenshots or a synthetic game:
//!
//! - [`WindowSource`]: a game window, live (Windows). The window is picked by
//!   title, executable, or automatically (the window in front, ignoring the
//!   desktop, terminals and Syrup's own windows).
//! - [`ScreenSource`]: the whole screen, live (Windows).
//! - [`VideoSource`]: a video file, decoded by `ffmpeg`.
//! - [`FolderSource`]: a folder of PNG or JPEG images.
//! - [`MemorySource`]: frames already in memory (tests, tools).
//!
//! Capture only ever reads pixels. Nothing in this crate sends input to any
//! window.

mod folder;
mod memory;
mod video;
#[cfg(windows)]
mod win32;
mod window;

use std::fmt;
use std::sync::Arc;

pub use folder::FolderSource;
pub use memory::MemorySource;
use syrup_core::{Frame, SourceInfo};
pub use video::{VideoInfo, VideoSource, probe_video};
pub use window::{
    ScreenSource, WindowInfo, WindowSelector, WindowSource, init_process, is_excluded,
    list_windows, pick_window,
};

/// What a source had for us this time.
#[derive(Debug)]
pub enum Capture {
    /// A new frame.
    Frame(Frame),
    /// Nothing right now (the window is minimised, or gone for a moment); ask again later.
    Waiting(String),
    /// The source has ended (the video is over).
    Ended,
}

#[derive(Debug)]
pub enum CaptureError {
    NotFound(String),
    Io(std::io::Error),
    Decode(String),
    Unsupported(String),
    Failed(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::NotFound(what) => write!(f, "not found: {what}"),
            CaptureError::Io(e) => write!(f, "{e}"),
            CaptureError::Decode(what) => write!(f, "could not decode: {what}"),
            CaptureError::Unsupported(what) => write!(f, "not supported here: {what}"),
            CaptureError::Failed(what) => write!(f, "capture failed: {what}"),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<std::io::Error> for CaptureError {
    fn from(e: std::io::Error) -> Self {
        CaptureError::Io(e)
    }
}

/// Anything that produces frames.
pub trait FrameSource: Send {
    /// What is being watched: its title, executable, path.
    fn info(&self) -> Arc<SourceInfo>;

    /// The next frame. Live sources capture now; recorded ones decode the next.
    fn next(&mut self) -> Result<Capture, CaptureError>;

    /// The rate the source naturally produces frames at (a video's rate; for
    /// live sources, how often it is worth asking).
    fn nominal_fps(&self) -> f32;

    /// How many frames there will be, when known.
    fn len_hint(&self) -> Option<u64> {
        None
    }

    /// Live sources run in real time; recorded ones as fast as they are read.
    fn is_live(&self) -> bool {
        self.info().is_live()
    }
}

impl FrameSource for Box<dyn FrameSource> {
    fn info(&self) -> Arc<SourceInfo> {
        (**self).info()
    }
    fn next(&mut self) -> Result<Capture, CaptureError> {
        (**self).next()
    }
    fn nominal_fps(&self) -> f32 {
        (**self).nominal_fps()
    }
    fn len_hint(&self) -> Option<u64> {
        (**self).len_hint()
    }
    fn is_live(&self) -> bool {
        (**self).is_live()
    }
}

/// Opens whatever `path` is: a folder of images, an image, or a video.
pub fn open_recording(
    path: &std::path::Path,
    fps: Option<f32>,
    max_width: Option<u32>,
) -> Result<Box<dyn FrameSource>, CaptureError> {
    if path.is_dir() {
        return Ok(Box::new(FolderSource::open(path, fps.unwrap_or(2.0))?));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return Ok(Box::new(FolderSource::single(path)?));
    }
    Ok(Box::new(VideoSource::open(path, fps, max_width)?))
}
