//! A captured frame and where it came from.

use std::sync::Arc;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// What produced the frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A game window, captured live.
    Window,
    /// The whole screen (or one monitor), captured live.
    Screen,
    /// A recorded video.
    Video,
    /// A folder of screenshots.
    Images,
    /// One of the synthetic test games.
    Synthetic,
}

/// Everything known about the source besides its pixels: the first clues to
/// which game this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub kind: SourceKind,
    pub window_title: Option<String>,
    /// The executable's file name, e.g. `MapleStory.exe`.
    pub executable: Option<String>,
    /// The executable's full path (reveals e.g. a Steam library folder).
    pub executable_path: Option<String>,
    pub process_id: Option<u32>,
    pub window_class: Option<String>,
    /// A file or folder, for recordings.
    pub path: Option<String>,
}

impl SourceInfo {
    pub fn new(kind: SourceKind) -> Self {
        SourceInfo {
            kind,
            window_title: None,
            executable: None,
            executable_path: None,
            process_id: None,
            window_class: None,
            path: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.window_title = Some(title.into());
        self
    }

    pub fn with_executable(mut self, exe: impl Into<String>) -> Self {
        self.executable = Some(exe.into());
        self
    }

    pub fn is_live(&self) -> bool {
        matches!(self.kind, SourceKind::Window | SourceKind::Screen)
    }
}

/// One frame: its pixels, when it was taken, and where it sits on the desktop.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Counts every captured frame, sampled or not.
    pub index: u64,
    /// Milliseconds since the source started. Every time Syrup reasons with
    /// comes from here, never from the wall clock, so replays and tests are
    /// deterministic.
    pub timestamp_ms: u64,
    pub image: Arc<RgbaImage>,
    /// Desktop position of the image's top left pixel (for placing the overlay).
    pub origin: (i32, i32),
    pub source: Arc<SourceInfo>,
}

impl Frame {
    pub fn new(index: u64, timestamp_ms: u64, image: RgbaImage, source: Arc<SourceInfo>) -> Self {
        Frame {
            index,
            timestamp_ms,
            image: Arc::new(image),
            origin: (0, 0),
            source,
        }
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn size(&self) -> (u32, u32) {
        self.image.dimensions()
    }
}
