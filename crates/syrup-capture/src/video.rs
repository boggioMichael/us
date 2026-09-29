//! A recorded video, decoded by `ffmpeg` into raw RGBA frames.
//!
//! `ffmpeg` and `ffprobe` must be on the `PATH`. Frames come out at a steady
//! rate (10 a second unless asked otherwise, never more than the video has),
//! optionally scaled down, with timestamps from the video's own clock.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;

use image::RgbaImage;
use syrup_core::Frame;
use syrup_core::frame::{SourceInfo, SourceKind};

use crate::{Capture, CaptureError, FrameSource};

#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub duration_s: Option<f32>,
}

/// Size, frame rate and length of a video, from `ffprobe`.
pub fn probe_video(path: &Path) -> Result<VideoInfo, CaptureError> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,r_frame_rate:format=duration",
        ])
        .args(["-of", "default=noprint_wrappers=1"])
        .arg(path)
        .output()
        .map_err(|e| CaptureError::Unsupported(format!("ffprobe is needed to read videos ({e})")))?;
    if !out.status.success() {
        return Err(CaptureError::Decode(format!(
            "{}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut width = 0;
    let mut height = 0;
    let mut fps = 0.0f32;
    let mut duration = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "width" => width = value.trim().parse().unwrap_or(0),
            "height" => height = value.trim().parse().unwrap_or(0),
            "avg_frame_rate" | "r_frame_rate" if fps <= 0.0 => fps = parse_rate(value.trim()).unwrap_or(0.0),
            "duration" => duration = value.trim().parse::<f32>().ok().filter(|d| *d > 0.0),
            _ => {}
        }
    }
    if width == 0 || height == 0 {
        return Err(CaptureError::Decode(format!("{}: no video stream", path.display())));
    }
    Ok(VideoInfo { width, height, fps: if fps > 0.0 { fps } else { 30.0 }, duration_s: duration })
}

fn parse_rate(s: &str) -> Option<f32> {
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b): (f32, f32) = (a.parse().ok()?, b.parse().ok()?);
            (b > 0.0 && a > 0.0).then_some(a / b)
        }
        None => s.parse().ok().filter(|v: &f32| *v > 0.0),
    }
}

pub struct VideoSource {
    path: PathBuf,
    child: Option<Child>,
    reader: Option<BufReader<ChildStdout>>,
    width: u32,
    height: u32,
    fps: f32,
    start_ms: u64,
    index: u64,
    total: Option<u64>,
    info: Arc<SourceInfo>,
}

impl VideoSource {
    /// `fps`: frames a second to decode (default: the video's, at most 10).
    /// `max_width`: scale larger videos down to this width.
    pub fn open(path: &Path, fps: Option<f32>, max_width: Option<u32>) -> Result<Self, CaptureError> {
        Self::open_at(path, fps, max_width, 0.0, None)
    }

    /// As [`open`](Self::open), starting `start_s` seconds in and lasting at most `length_s`.
    pub fn open_at(
        path: &Path,
        fps: Option<f32>,
        max_width: Option<u32>,
        start_s: f32,
        length_s: Option<f32>,
    ) -> Result<Self, CaptureError> {
        if !path.is_file() {
            return Err(CaptureError::NotFound(path.display().to_string()));
        }
        let probe = probe_video(path)?;
        let fps = fps.unwrap_or(probe.fps.min(10.0)).clamp(0.1, probe.fps.max(0.1));
        let (width, height) = scaled_size(probe.width, probe.height, max_width);
        let mut filter = format!("fps={fps}");
        if (width, height) != (probe.width, probe.height) {
            filter.push_str(&format!(",scale={width}:{height}:flags=area"));
        }
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if start_s > 0.0 {
            cmd.args(["-ss", &format!("{start_s:.3}")]);
        }
        cmd.arg("-i").arg(path);
        if let Some(len) = length_s {
            cmd.args(["-t", &format!("{len:.3}")]);
        }
        cmd.args(["-an", "-vf", &filter, "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CaptureError::Unsupported(format!("ffmpeg is needed to read videos ({e})")))?;
        let stdout = child.stdout.take().ok_or_else(|| CaptureError::Failed("ffmpeg has no output".into()))?;
        let playable = match (probe.duration_s, length_s) {
            (Some(d), Some(l)) => Some((d - start_s).min(l)),
            (Some(d), None) => Some(d - start_s),
            (None, l) => l,
        };
        let total = playable.filter(|s| *s > 0.0).map(|s| (s * fps).floor() as u64);
        let mut info = SourceInfo::new(SourceKind::Video);
        info.path = Some(path.display().to_string());
        info.window_title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        Ok(VideoSource {
            path: path.to_path_buf(),
            child: Some(child),
            reader: Some(BufReader::with_capacity(1 << 20, stdout)),
            width,
            height,
            fps,
            start_ms: (start_s * 1000.0) as u64,
            index: 0,
            total,
            info: Arc::new(info),
        })
    }

    /// Titles the source (what recognition sees as the window title).
    pub fn with_title(mut self, title: &str) -> Self {
        let mut info = (*self.info).clone();
        info.window_title = Some(title.to_string());
        self.info = Arc::new(info);
        self
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The size after scaling down to `max_width`, even in both dimensions (as encoders like).
fn scaled_size(w: u32, h: u32, max_width: Option<u32>) -> (u32, u32) {
    match max_width {
        Some(mw) if mw >= 2 && w > mw => {
            let nw = mw & !1;
            let nh = ((h as f64 * nw as f64 / w as f64).round() as u32).max(2) & !1;
            (nw, nh)
        }
        _ => (w, h),
    }
}

impl FrameSource for VideoSource {
    fn info(&self) -> Arc<SourceInfo> {
        self.info.clone()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(Capture::Ended);
        };
        let mut buf = vec![0u8; self.width as usize * self.height as usize * 4];
        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.reader = None;
                if let Some(mut child) = self.child.take() {
                    let _ = child.wait();
                }
                return Ok(Capture::Ended);
            }
            Err(e) => return Err(CaptureError::Io(e)),
        }
        let image = RgbaImage::from_raw(self.width, self.height, buf)
            .ok_or_else(|| CaptureError::Decode("short frame".into()))?;
        let ts = self.start_ms + (self.index as f64 * 1000.0 / self.fps as f64).round() as u64;
        let frame = Frame::new(self.index, ts, image, self.info.clone());
        self.index += 1;
        Ok(Capture::Frame(frame))
    }

    fn nominal_fps(&self) -> f32 {
        self.fps
    }

    fn len_hint(&self) -> Option<u64> {
        self.total
    }
}

impl Drop for VideoSource {
    fn drop(&mut self) {
        self.reader = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_sizes() {
        assert_eq!(parse_rate("30000/1001").map(|r| (r * 100.0).round()), Some(2997.0));
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("25"), Some(25.0));
        assert_eq!(scaled_size(1920, 1080, Some(1280)), (1280, 720));
        assert_eq!(scaled_size(1366, 768, Some(1001)), (1000, 562));
        assert_eq!(scaled_size(800, 600, Some(1280)), (800, 600));
    }

    /// Encodes a short clip with ffmpeg (when it is installed) and reads it back.
    #[test]
    fn a_video_decodes_with_its_own_clock() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg not installed: skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        let ok = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=20:duration=2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&clip)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("ffmpeg could not encode: skipped");
            return;
        }
        let mut src = VideoSource::open(&clip, Some(5.0), Some(160)).unwrap();
        assert_eq!(src.size(), (160, 120));
        let mut stamps = Vec::new();
        while let Capture::Frame(f) = src.next().unwrap() {
            assert_eq!(f.size(), (160, 120));
            stamps.push(f.timestamp_ms);
        }
        assert_eq!(stamps.len(), 10, "{stamps:?}");
        assert_eq!(&stamps[..3], &[0, 200, 400]);
    }
}
