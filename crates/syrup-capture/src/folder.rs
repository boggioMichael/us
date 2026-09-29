//! A folder of screenshots, in name order.
//!
//! Timestamps come from the file names when they carry one (`12500ms.png`,
//! `frame-12500ms.png`, `shot_t12500.png`); otherwise frames are spaced at the
//! folder's frame rate.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use syrup_core::Frame;
use syrup_core::frame::{SourceInfo, SourceKind};

use crate::{Capture, CaptureError, FrameSource};

pub struct FolderSource {
    files: Vec<PathBuf>,
    next: usize,
    fps: f32,
    info: Arc<SourceInfo>,
}

impl FolderSource {
    pub fn open(dir: &Path, fps: f32) -> Result<Self, CaptureError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
            })
            .collect();
        if files.is_empty() {
            return Err(CaptureError::NotFound(format!("no PNG or JPEG images in {}", dir.display())));
        }
        files.sort_by_key(|p| natural_key(p));
        let mut info = SourceInfo::new(SourceKind::Images);
        info.path = Some(dir.display().to_string());
        info.window_title = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        Ok(FolderSource { files, next: 0, fps: fps.max(0.01), info: Arc::new(info) })
    }

    pub fn single(file: &Path) -> Result<Self, CaptureError> {
        if !file.is_file() {
            return Err(CaptureError::NotFound(file.display().to_string()));
        }
        let mut info = SourceInfo::new(SourceKind::Images);
        info.path = Some(file.display().to_string());
        info.window_title = file.file_stem().map(|n| n.to_string_lossy().into_owned());
        Ok(FolderSource { files: vec![file.to_path_buf()], next: 0, fps: 1.0, info: Arc::new(info) })
    }

    /// Titles the source (what recognition sees as the window title).
    pub fn with_title(mut self, title: &str) -> Self {
        let mut info = (*self.info).clone();
        info.window_title = Some(title.to_string());
        self.info = Arc::new(info);
        self
    }
}

impl FrameSource for FolderSource {
    fn info(&self) -> Arc<SourceInfo> {
        self.info.clone()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        let Some(path) = self.files.get(self.next).cloned() else {
            return Ok(Capture::Ended);
        };
        let index = self.next as u64;
        self.next += 1;
        let image =
            image::open(&path).map_err(|e| CaptureError::Decode(format!("{}: {e}", path.display())))?.to_rgba8();
        let ts = timestamp_from_name(&path).unwrap_or_else(|| (index as f64 * 1000.0 / self.fps as f64).round() as u64);
        Ok(Capture::Frame(Frame::new(index, ts, image, self.info.clone())))
    }

    fn nominal_fps(&self) -> f32 {
        self.fps
    }

    fn len_hint(&self) -> Option<u64> {
        Some(self.files.len() as u64)
    }
}

/// Names sort with their numbers compared as numbers: `frame-9` before `frame-10`.
fn natural_key(path: &Path) -> Vec<(String, u64)> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((std::mem::take(&mut text), digits.parse().unwrap_or(u64::MAX)));
                digits.clear();
            }
            text.push(c);
        }
    }
    out.push((text, digits.parse().unwrap_or(0)));
    out
}

/// `12500ms`, `t12500` or `-12500ms` in a file name: milliseconds into the recording.
pub(crate) fn timestamp_from_name(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    let bytes = stem.as_bytes();
    // "<digits>ms"
    if let Some(pos) = stem.rfind("ms") {
        let digits: String = stem[..pos].chars().rev().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && pos + 2 == stem.len() {
            return digits.chars().rev().collect::<String>().parse().ok();
        }
    }
    // "t<digits>" at the end
    let digits: String = stem.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    let start = bytes.len() - digits.len();
    if !digits.is_empty()
        && start > 0
        && bytes[start - 1] == b't'
        && (start < 2 || !bytes[start - 2].is_ascii_alphabetic())
    {
        return digits.chars().rev().collect::<String>().parse().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_give_times_and_order() {
        assert_eq!(timestamp_from_name(Path::new("a/12500ms.png")), Some(12500));
        assert_eq!(timestamp_from_name(Path::new("frame-40ms.png")), Some(40));
        assert_eq!(timestamp_from_name(Path::new("shot_t300.png")), Some(300));
        assert_eq!(timestamp_from_name(Path::new("frame-0003.png")), None);
        assert_eq!(timestamp_from_name(Path::new("boost300.png")), None);
        let mut v = vec![PathBuf::from("f10.png"), PathBuf::from("f9.png"), PathBuf::from("f100.png")];
        v.sort_by_key(|p| natural_key(p));
        assert_eq!(v, vec![PathBuf::from("f9.png"), PathBuf::from("f10.png"), PathBuf::from("f100.png")]);
    }

    #[test]
    fn a_folder_plays_in_order() {
        let dir = tempfile::tempdir().unwrap();
        for (i, name) in ["b-2.png", "a-10.png", "b-1.png"].iter().enumerate() {
            let img = image::RgbaImage::from_pixel(4, 3, image::Rgba([i as u8 * 50, 0, 0, 255]));
            img.save(dir.path().join(name)).unwrap();
        }
        let mut src = FolderSource::open(dir.path(), 4.0).unwrap();
        assert_eq!(src.len_hint(), Some(3));
        let mut seen = Vec::new();
        while let Capture::Frame(f) = src.next().unwrap() {
            seen.push((f.index, f.timestamp_ms, f.image.get_pixel(0, 0).0[0]));
        }
        // a-10, b-1, b-2 at 4 fps.
        assert_eq!(seen, vec![(0, 0, 50), (1, 250, 100), (2, 500, 0)]);
    }
}
