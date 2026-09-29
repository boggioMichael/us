//! Reading text on screen.
//!
//! OCR is expensive, so it is scheduled: the whole screen now and then (and
//! whenever the scene changes), and each interface region again when its
//! pixels change, a few regions at a time. In live use the reads run on a
//! worker thread and their results arrive a frame or two later; replays and
//! tests read synchronously, so they are reproducible.
//!
//! Engines: Windows' own OCR (fast, reads whole frames), Tesseract (a
//! subprocess, so it reads likely text lines one at a time), or none, in
//! which case perception says so and everything else still works.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use image::RgbaImage;
use syrup_core::observation::TextItem;
use syrup_core::{Confidence, Frame, Rect};

use crate::pixels::{luma, thumb_difference, thumbnail};

#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub rect: Rect,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    pub rect: Rect,
    pub words: Vec<OcrWord>,
    pub confidence: f32,
}

/// What the picture handed to the engine holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextHint {
    /// One line of text.
    Line,
    /// A block of lines.
    Block,
    /// Text scattered anywhere (a whole frame).
    Sparse,
}

pub trait OcrEngine: Send + Sync {
    fn name(&self) -> &'static str;
    /// The lines in `img`, with boxes relative to it.
    fn read(&self, img: &RgbaImage, hint: TextHint) -> Result<Vec<OcrLine>, String>;
    /// Fast and layout-aware enough to be handed whole frames.
    fn reads_whole_frames(&self) -> bool {
        false
    }
    fn is_available(&self) -> bool {
        true
    }
}

/// No OCR: perception reports that it cannot read.
pub struct NoOcr;

impl OcrEngine for NoOcr {
    fn name(&self) -> &'static str {
        "none"
    }
    fn read(&self, _: &RgbaImage, _: TextHint) -> Result<Vec<OcrLine>, String> {
        Ok(Vec::new())
    }
    fn is_available(&self) -> bool {
        false
    }
}

/// Tesseract, as a subprocess (one per read). HUD text is small, colourful
/// and drawn over pictures, so each crop is enlarged until its text is about
/// 40 pixels tall and framed in its own background colour before Tesseract
/// sees it; whole frames are passed as they are.
pub struct TesseractOcr;

impl TesseractOcr {
    pub fn available() -> bool {
        syrup::ocr::is_ocr_available()
    }

    fn binary() -> &'static str {
        static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        BIN.get_or_init(|| {
            if let Some(p) = std::env::var_os("TESSERACT_PATH") {
                return p.to_string_lossy().into_owned();
            }
            #[cfg(windows)]
            for p in
                [r"C:\Program Files\Tesseract-OCR\tesseract.exe", r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe"]
            {
                if std::path::Path::new(p).exists() {
                    return p.to_string();
                }
            }
            "tesseract".to_string()
        })
    }
}

/// The median colour of an image's outermost pixels (its background).
fn border_color(img: &RgbaImage) -> [u8; 3] {
    let (w, h) = img.dimensions();
    let mut ch: [Vec<u8>; 3] = Default::default();
    let mut add = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        for c in 0..3 {
            ch[c].push(p[c]);
        }
    };
    for x in 0..w {
        add(x, 0);
        add(x, h - 1);
    }
    for y in 0..h {
        add(0, y);
        add(w - 1, y);
    }
    let mut out = [0u8; 3];
    for c in 0..3 {
        let mid = ch[c].len() / 2;
        out[c] = *ch[c].select_nth_unstable(mid).1;
    }
    out
}

impl OcrEngine for TesseractOcr {
    fn name(&self) -> &'static str {
        "tesseract"
    }

    fn read(&self, img: &RgbaImage, hint: TextHint) -> Result<Vec<OcrLine>, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let (w, h) = img.dimensions();
        if w < 4 || h < 4 {
            return Ok(Vec::new());
        }
        let (psm, k) = match hint {
            TextHint::Line => (7, (40.0 / h as f32).clamp(1.0, 4.0)),
            TextHint::Block => (6, (80.0 / h as f32).clamp(1.0, 3.0)),
            TextHint::Sparse => (11, 1.0),
        };
        let (sw, sh) = (((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1));
        let margin = if hint == TextHint::Sparse { 0 } else { 12 };
        let bg = border_color(img);
        let mut canvas =
            RgbaImage::from_pixel(sw + 2 * margin, sh + 2 * margin, image::Rgba([bg[0], bg[1], bg[2], 255]));
        let scaled = if k > 1.0 {
            image::imageops::resize(img, sw, sh, image::imageops::FilterType::CatmullRom)
        } else {
            img.clone()
        };
        image::imageops::replace(&mut canvas, &scaled, margin as i64, margin as i64);
        let path = std::env::temp_dir().join(format!(
            "syrup-ocr-{}-{}.png",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        canvas.save(&path).map_err(|e| e.to_string())?;
        let out = std::process::Command::new(Self::binary())
            .arg(&path)
            .arg("stdout")
            .args(["--psm", &psm.to_string(), "-c", "preserve_interword_spaces=1", "tsv"])
            .env("OMP_THREAD_LIMIT", "1")
            .output();
        let _ = std::fs::remove_file(&path);
        let out = out.map_err(|e| e.to_string())?;
        let tsv = String::from_utf8_lossy(&out.stdout);
        let mut words = Vec::new();
        for line in tsv.lines().skip(1) {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 12 || f[11].trim().is_empty() {
                continue;
            }
            let conf: f32 = f[10].trim().parse().unwrap_or(-1.0);
            if conf < 30.0 {
                continue;
            }
            let num = |i: usize| f[i].trim().parse::<f32>().unwrap_or(0.0);
            let (x, y, ww, hh) = (num(6) - margin as f32, num(7) - margin as f32, num(8), num(9));
            let rect = Rect::new(
                (x / k).floor().max(0.0) as i32,
                (y / k).floor().max(0.0) as i32,
                (ww / k).ceil() as u32,
                (hh / k).ceil() as u32,
            );
            words.push(OcrWord { text: f[11].trim().to_string(), rect, confidence: conf / 100.0 });
        }
        Ok(group_lines(words))
    }

    fn is_available(&self) -> bool {
        TesseractOcr::available()
    }
}

/// Words on the same baseline become a line, left to right.
pub fn group_lines(mut words: Vec<OcrWord>) -> Vec<OcrLine> {
    words.sort_by_key(|w| (w.rect.y + w.rect.h as i32 / 2, w.rect.x));
    let mut lines: Vec<Vec<OcrWord>> = Vec::new();
    for w in words {
        let cy = w.rect.y + w.rect.h as i32 / 2;
        let found = lines.iter_mut().find(|l| {
            let r = l.iter().fold(l[0].rect, |a, b| a.union(&b.rect));
            cy >= r.y && cy <= r.bottom() && w.rect.x <= r.right() + (r.h as i32 * 3)
        });
        match found {
            Some(l) => l.push(w),
            None => lines.push(vec![w]),
        }
    }
    lines
        .into_iter()
        .map(|mut l| {
            l.sort_by_key(|w| w.rect.x);
            let rect = l.iter().skip(1).fold(l[0].rect, |a, b| a.union(&b.rect));
            let confidence = l.iter().map(|w| w.confidence).sum::<f32>() / l.len() as f32;
            let text = l.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            OcrLine { text, rect, words: l, confidence }
        })
        .collect()
}

#[cfg(windows)]
mod windows_ocr {
    use image::RgbaImage;
    use syrup_core::Rect;
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine as WinEngine;
    use windows::Storage::Streams::DataWriter;

    use super::{OcrEngine, OcrLine, OcrWord, TextHint};

    /// The OCR engine built into Windows (needs a language pack, usually present).
    pub struct WindowsOcr {
        engine: WinEngine,
        max_dim: u32,
    }

    impl WindowsOcr {
        pub fn new() -> Option<Self> {
            let engine = WinEngine::TryCreateFromUserProfileLanguages().ok()?;
            let max_dim = WinEngine::MaxImageDimension().unwrap_or(2600);
            Some(WindowsOcr { engine, max_dim })
        }

        fn bitmap(img: &RgbaImage) -> Option<SoftwareBitmap> {
            let (w, h) = img.dimensions();
            let mut bgra = Vec::with_capacity((w * h * 4) as usize);
            for p in img.pixels() {
                bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
            let writer = DataWriter::new().ok()?;
            writer.WriteBytes(&bgra).ok()?;
            let buffer = writer.DetachBuffer().ok()?;
            let bitmap =
                SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32).ok()?;
            SoftwareBitmap::ConvertWithAlpha(&bitmap, BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied).ok()
        }
    }

    impl OcrEngine for WindowsOcr {
        fn name(&self) -> &'static str {
            "windows"
        }

        fn read(&self, img: &RgbaImage, hint: TextHint) -> Result<Vec<OcrLine>, String> {
            // Small text reads better enlarged; whole frames are read as they are.
            let (w, h) = img.dimensions();
            let mut k = if hint == TextHint::Sparse || h >= 60 { 1.0f32 } else { 2.0 };
            let biggest = w.max(h) as f32 * k;
            if biggest > self.max_dim as f32 {
                k *= self.max_dim as f32 / biggest;
            }
            let scaled;
            let src = if (k - 1.0).abs() > 1e-3 {
                scaled = image::imageops::resize(
                    img,
                    ((w as f32 * k) as u32).max(1),
                    ((h as f32 * k) as u32).max(1),
                    image::imageops::FilterType::Triangle,
                );
                &scaled
            } else {
                img
            };
            let bitmap = Self::bitmap(src).ok_or("could not make a bitmap")?;
            let result = self.engine.RecognizeAsync(&bitmap).and_then(|op| op.get()).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for line in result.Lines().map_err(|e| e.to_string())? {
                let mut words = Vec::new();
                for word in line.Words().map_err(|e| e.to_string())? {
                    let text = word.Text().map(|t| t.to_string()).unwrap_or_default();
                    let r = word.BoundingRect().map_err(|e| e.to_string())?;
                    let rect = Rect::new(
                        (r.X / k) as i32,
                        (r.Y / k) as i32,
                        (r.Width / k).ceil() as u32,
                        (r.Height / k).ceil() as u32,
                    );
                    if !text.trim().is_empty() {
                        words.push(OcrWord { text, rect, confidence: 0.85 });
                    }
                }
                if words.is_empty() {
                    continue;
                }
                let rect = words.iter().skip(1).fold(words[0].rect, |a, b| a.union(&b.rect));
                let text = line
                    .Text()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|_| words.iter().map(|w| w.text.clone()).collect::<Vec<_>>().join(" "));
                out.push(OcrLine { text, rect, words, confidence: 0.85 });
            }
            Ok(out)
        }

        fn reads_whole_frames(&self) -> bool {
            true
        }
    }
}

#[cfg(windows)]
pub use windows_ocr::WindowsOcr;

/// The best engine this machine has: Windows OCR, then Tesseract, then none.
pub fn best_engine() -> Arc<dyn OcrEngine> {
    #[cfg(windows)]
    if let Some(e) = WindowsOcr::new() {
        return Arc::new(e);
    }
    if TesseractOcr::available() {
        return Arc::new(TesseractOcr);
    }
    Arc::new(NoOcr)
}

/// An engine by name: `windows`, `tesseract`, `none`, or `auto`.
pub fn engine_named(name: &str) -> Option<Arc<dyn OcrEngine>> {
    match name {
        "auto" => Some(best_engine()),
        "none" | "off" => Some(Arc::new(NoOcr)),
        "tesseract" => TesseractOcr::available().then(|| Arc::new(TesseractOcr) as Arc<dyn OcrEngine>),
        #[cfg(windows)]
        "windows" => WindowsOcr::new().map(|e| Arc::new(e) as Arc<dyn OcrEngine>),
        _ => None,
    }
}

/// Where text probably is: rows of short, strong strokes. Returns candidate
/// line boxes (frame pixels) with a score, best first.
pub fn find_text_lines(img: &RgbaImage, area: Rect) -> Vec<(Rect, f32)> {
    let Some(area) = area.clip(img.width(), img.height()) else {
        return Vec::new();
    };
    let (aw, ah) = (area.w as usize, area.h as usize);
    if aw < 16 || ah < 8 {
        return Vec::new();
    }
    const CELL: usize = 3;
    let (gw, gh) = (aw / CELL, ah / CELL);
    let raw = img.as_raw();
    let fw = img.width() as usize;
    let l = |x: usize, y: usize| {
        let i = ((area.y as usize + y) * fw + area.x as usize + x) * 4;
        luma(raw[i], raw[i + 1], raw[i + 2]) as i32
    };
    let mut cells = vec![0u8; gw * gh];
    for y in 1..ah - 1 {
        for x in 1..aw - 1 {
            let gx = (l(x + 1, y) - l(x - 1, y)).abs();
            let gy = (l(x, y + 1) - l(x, y - 1)).abs();
            if gx.max(gy) >= 70 {
                let (cx, cy) = ((x / CELL).min(gw - 1), (y / CELL).min(gh - 1));
                cells[cy * gw + cx] = cells[cy * gw + cx].saturating_add(1);
            }
        }
    }
    let on: Vec<bool> = cells.iter().map(|c| *c >= 2).collect();
    // Close gaps between letters and words along rows.
    let mut closed = on.clone();
    for y in 0..gh {
        let mut last: Option<usize> = None;
        for x in 0..gw {
            if on[y * gw + x] {
                if let Some(l) = last
                    && x - l <= 3
                {
                    for k in l..x {
                        closed[y * gw + k] = true;
                    }
                }
                last = Some(x);
            }
        }
    }
    let comps = crate::stability::cell_components(gw, gh, &closed, 3);
    let mut out = Vec::new();
    for (x, y, w, h) in comps {
        let (pw, ph) = (w * CELL, h * CELL);
        if !(6..=96).contains(&ph) || pw < 12 || (pw as f32) < ph as f32 * 1.1 {
            continue;
        }
        let density =
            (y..y + h).flat_map(|yy| (x..x + w).map(move |xx| (xx, yy))).filter(|(xx, yy)| on[yy * gw + xx]).count()
                as f32
                / (w * h) as f32;
        if density < 0.25 {
            continue;
        }
        let rect = Rect::new(area.x + (x * CELL) as i32, area.y + (y * CELL) as i32, pw as u32, ph as u32);
        out.push((rect, density * (pw as f32).sqrt()));
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[derive(Debug, Clone)]
pub struct TextReaderConfig {
    /// Read the whole screen this often (and on every scene change).
    pub full_every_ms: u64,
    /// Read a region again at most this often (when its pixels changed).
    pub region_every_ms: u64,
    /// Regions read per analysed frame, at most.
    pub max_region_reads: usize,
    /// Candidate lines read per whole-screen read, for engines that read lines.
    pub max_line_reads: usize,
    /// Read on a worker thread (live) or right away (replays, tests).
    pub asynchronous: bool,
}

impl Default for TextReaderConfig {
    fn default() -> Self {
        TextReaderConfig {
            full_every_ms: 3000,
            region_every_ms: 700,
            max_region_reads: 3,
            max_line_reads: 10,
            asynchronous: false,
        }
    }
}

/// A region to read, as the analyser sees it now.
#[derive(Debug, Clone)]
pub struct RegionToRead {
    pub id: u32,
    pub rect: Rect,
    pub content: u64,
    pub is_bar: bool,
}

enum Job {
    Full {
        image: Arc<RgbaImage>,
        ts: u64,
    },
    /// A region, as one or more crops (a bar: its label, itself, its number).
    Region {
        id: u32,
        content: u64,
        crops: Vec<(RgbaImage, (i32, i32))>,
        ts: u64,
    },
}

enum Done {
    Full { items: Vec<TextItem>, ts: u64 },
    Region { id: u32, content: u64, items: Vec<TextItem>, ts: u64 },
}

struct RegionRead {
    content: u64,
    at_ms: u64,
    items: Vec<TextItem>,
}

pub struct TextReader {
    engine: Arc<dyn OcrEngine>,
    cfg: TextReaderConfig,
    last_full_ms: Option<u64>,
    full_items: Vec<TextItem>,
    full_at_ms: u64,
    regions: HashMap<u32, RegionRead>,
    pending_full: bool,
    pending_regions: HashSet<u32>,
    jobs: Option<Sender<Job>>,
    done: Option<Receiver<Done>>,
    fresh_full: bool,
    fresh_regions: HashSet<u32>,
    /// A thumbnail of the frame last read whole: the screen changing a lot
    /// since (a fade, a slow transition) calls for another read.
    full_thumb: Vec<u8>,
    /// Reads performed (for telemetry).
    pub reads: u64,
}

fn run_job(engine: &dyn OcrEngine, job: Job, max_lines: usize) -> Done {
    match job {
        Job::Full { image, ts } => {
            let mut items = Vec::new();
            if engine.reads_whole_frames() {
                if let Ok(lines) = engine.read(&image, TextHint::Sparse) {
                    items.extend(lines.into_iter().map(|l| item(l, (0, 0), engine.name())));
                }
            } else {
                let full = Rect::new(0, 0, image.width(), image.height());
                for (r, _) in find_text_lines(&image, full).into_iter().take(max_lines) {
                    let crop_rect = r.inflate(3).clip(image.width(), image.height()).unwrap_or(r);
                    let crop = image::imageops::crop_imm(
                        &*image,
                        crop_rect.x as u32,
                        crop_rect.y as u32,
                        crop_rect.w,
                        crop_rect.h,
                    )
                    .to_image();
                    if let Ok(lines) = engine.read(&crop, TextHint::Line) {
                        items.extend(lines.into_iter().map(|l| item(l, (crop_rect.x, crop_rect.y), engine.name())));
                    }
                }
            }
            Done::Full { items: clean(items), ts }
        }
        Job::Region { id, content, crops, ts } => {
            let mut items = Vec::new();
            for (image, origin) in crops {
                let hint = if image.height() <= 40 { TextHint::Line } else { TextHint::Block };
                if let Ok(lines) = engine.read(&image, hint) {
                    items.extend(lines.into_iter().map(|l| item(l, origin, engine.name())));
                }
            }
            Done::Region { id, content, items: clean(items), ts }
        }
    }
}

fn item(l: OcrLine, (ox, oy): (i32, i32), engine: &str) -> TextItem {
    TextItem {
        text: l.text.trim().to_string(),
        rect: Rect::new(l.rect.x + ox, l.rect.y + oy, l.rect.w, l.rect.h),
        region: None,
        confidence: Confidence::new(l.confidence),
        engine: engine.to_string(),
        fresh: true,
    }
}

/// Drops what OCR makes of noise: lines with no letters or digits, or mostly symbols.
fn clean(items: Vec<TextItem>) -> Vec<TextItem> {
    items
        .into_iter()
        .filter(|t| {
            let alnum = t.text.chars().filter(|c| c.is_alphanumeric()).count();
            let total = t.text.chars().filter(|c| !c.is_whitespace()).count();
            alnum >= 1
                && (alnum as f32) >= total as f32 * 0.5
                && (alnum >= 2 || t.text.chars().any(|c| c.is_ascii_digit()))
        })
        .collect()
}

impl TextReader {
    pub fn new(engine: Arc<dyn OcrEngine>, cfg: TextReaderConfig) -> Self {
        let (jobs, done) = if cfg.asynchronous && engine.is_available() {
            let (job_tx, job_rx) = channel::<Job>();
            let (done_tx, done_rx) = channel::<Done>();
            let eng = engine.clone();
            let max_lines = cfg.max_line_reads;
            std::thread::Builder::new()
                .name("syrup-ocr".into())
                .spawn(move || {
                    for job in job_rx {
                        if done_tx.send(run_job(&*eng, job, max_lines)).is_err() {
                            break;
                        }
                    }
                })
                .ok();
            (Some(job_tx), Some(done_rx))
        } else {
            (None, None)
        };
        TextReader {
            engine,
            cfg,
            last_full_ms: None,
            full_items: Vec::new(),
            full_at_ms: 0,
            regions: HashMap::new(),
            pending_full: false,
            pending_regions: HashSet::new(),
            jobs,
            done,
            fresh_full: false,
            fresh_regions: HashSet::new(),
            full_thumb: Vec::new(),
            reads: 0,
        }
    }

    pub fn engine_name(&self) -> &'static str {
        self.engine.name()
    }

    pub fn available(&self) -> bool {
        self.engine.is_available()
    }

    pub fn reset(&mut self) {
        self.last_full_ms = None;
        self.full_items.clear();
        self.regions.clear();
        self.full_thumb.clear();
    }

    fn submit(&mut self, job: Job) {
        self.reads += 1;
        match &self.jobs {
            Some(tx) => {
                match &job {
                    Job::Full { .. } => self.pending_full = true,
                    Job::Region { id, .. } => {
                        self.pending_regions.insert(*id);
                    }
                }
                let _ = tx.send(job);
            }
            None => {
                let done = run_job(&*self.engine, job, self.cfg.max_line_reads);
                self.accept(done);
            }
        }
    }

    fn accept(&mut self, done: Done) {
        match done {
            Done::Full { items, ts } => {
                self.pending_full = false;
                self.full_items = items;
                self.full_at_ms = ts;
                self.fresh_full = true;
            }
            Done::Region { id, content, items, ts } => {
                self.pending_regions.remove(&id);
                self.regions.insert(id, RegionRead { content, at_ms: ts, items });
                self.fresh_regions.insert(id);
            }
        }
    }

    /// Schedules reads for this frame and returns everything currently known
    /// to be written on screen.
    pub fn update(&mut self, frame: &Frame, regions: &[RegionToRead], force_full: bool) -> Vec<TextItem> {
        self.fresh_full = false;
        self.fresh_regions.clear();
        if let Some(rx) = &self.done {
            let finished: Vec<Done> = rx.try_iter().collect();
            for d in finished {
                self.accept(d);
            }
        }
        if !self.engine.is_available() {
            return Vec::new();
        }
        let now = frame.timestamp_ms;
        let (fw, fh) = frame.size();
        let thumb = thumbnail(&frame.image, 48, 27);
        // A different screen since the last whole read (however gradually it came).
        let moved_on = !self.full_thumb.is_empty()
            && thumb_difference(&thumb, &self.full_thumb) > 0.06
            && self.last_full_ms.is_some_and(|t| now.saturating_sub(t) >= 400);
        let full_due =
            self.last_full_ms.is_none_or(|t| now.saturating_sub(t) >= self.cfg.full_every_ms) || force_full || moved_on;
        if full_due && !self.pending_full {
            self.last_full_ms = Some(now);
            self.full_thumb = thumb;
            self.submit(Job::Full { image: frame.image.clone(), ts: now });
        }
        // Regions: never read first, then changed ones; bars before other things; small before big.
        let frame_area = (fw as u64 * fh as u64).max(1);
        let mut due: Vec<&RegionToRead> = regions
            .iter()
            .filter(|r| r.rect.area() * 100 / frame_area <= 35 && r.rect.w >= 8 && r.rect.h >= 6)
            .filter(|r| !self.pending_regions.contains(&r.id))
            .filter(|r| match self.regions.get(&r.id) {
                None => true,
                Some(read) => read.content != r.content && now.saturating_sub(read.at_ms) >= self.cfg.region_every_ms,
            })
            .collect();
        due.sort_by_key(|r| (self.regions.contains_key(&r.id), !r.is_bar, r.rect.area()));
        for r in due.into_iter().take(self.cfg.max_region_reads) {
            let rects: Vec<Rect> = if r.is_bar {
                // A bar's label is before it, its number on it or after it.
                let h = r.rect.h.max(8) as i32;
                let (y, hh) = (r.rect.y - h / 2, r.rect.h + h as u32);
                vec![
                    Rect::new(r.rect.x - 6 * h, y, 6 * h as u32, hh),
                    Rect::new(r.rect.x - 2, r.rect.y - 2, r.rect.w + 4, r.rect.h + 4),
                    Rect::new(r.rect.right(), y, 8 * h as u32, hh),
                ]
            } else {
                vec![r.rect.inflate(3)]
            };
            let crops: Vec<(RgbaImage, (i32, i32))> = rects
                .into_iter()
                .filter_map(|c| c.clip(fw, fh))
                .filter(|c| c.w >= 6 && c.h >= 6)
                .map(|c| {
                    (image::imageops::crop_imm(&*frame.image, c.x as u32, c.y as u32, c.w, c.h).to_image(), (c.x, c.y))
                })
                .collect();
            if !crops.is_empty() {
                self.submit(Job::Region { id: r.id, content: r.content, crops, ts: now });
            }
        }
        // Forget regions that are gone; let whole-screen text expire.
        let alive: HashSet<u32> = regions.iter().map(|r| r.id).collect();
        self.regions.retain(|id, _| alive.contains(id));
        if now.saturating_sub(self.full_at_ms) > self.cfg.full_every_ms * 2 + 1000 {
            self.full_items.clear();
        }
        self.current()
    }

    fn current(&self) -> Vec<TextItem> {
        let mut out: Vec<TextItem> = Vec::new();
        let mut ids: Vec<&u32> = self.regions.keys().collect();
        ids.sort();
        for id in ids {
            let read = &self.regions[id];
            let fresh = self.fresh_regions.contains(id);
            for t in &read.items {
                out.push(TextItem { fresh, ..t.clone() });
            }
        }
        let from_regions = out.len();
        for t in &self.full_items {
            // Region reads are newer and closer; whole-screen text they already cover is dropped.
            if out[..from_regions].iter().any(|o| o.rect.iou(&t.rect) > 0.3 || o.rect.contains_rect(&t.rect)) {
                continue;
            }
            out.push(TextItem { fresh: self.fresh_full, ..t.clone() });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syrup_paint::{FontStyle, Painter, rgb};

    #[test]
    fn text_lines_are_found_where_text_is() {
        let mut img = RgbaImage::from_pixel(640, 360, image::Rgba([30, 60, 40, 255]));
        let mut p = Painter::new(&mut img);
        p.text(40.0, 30.0, "SCORE 2 - 1", FontStyle::bold(22.0), rgb(250, 250, 250));
        p.text(400.0, 300.0, "TIME 0:07", FontStyle::bold(22.0), rgb(250, 250, 250));
        p.fill_rect(300, 150, 60, 60, rgb(200, 30, 30));
        let lines = find_text_lines(&img, Rect::new(0, 0, 640, 360));
        assert!(lines.len() >= 2, "{lines:?}");
        let has = |x: i32, y: i32| lines.iter().any(|(r, _)| r.contains(x, y));
        assert!(has(80, 42), "{lines:?}");
        assert!(has(450, 312), "{lines:?}");
        assert!(!has(330, 180), "a plain square is not text: {lines:?}");
    }

    #[test]
    fn words_group_into_lines() {
        let w = |t: &str, x: i32, y: i32| OcrWord { text: t.into(), rect: Rect::new(x, y, 30, 12), confidence: 0.9 };
        let lines = group_lines(vec![w("0:07", 90, 11), w("TIME", 50, 10), w("SCORE", 10, 100)]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "TIME 0:07");
        assert_eq!(lines[1].text, "SCORE");
    }

    #[test]
    fn tesseract_reads_a_hud_line_when_installed() {
        if !TesseractOcr::available() {
            eprintln!("tesseract not installed: skipped");
            return;
        }
        let mut img = RgbaImage::from_pixel(200, 40, image::Rgba([20, 20, 30, 255]));
        Painter::new(&mut img).text(10.0, 8.0, "AMMO 24", FontStyle::bold(20.0), rgb(230, 230, 160));
        let lines = TesseractOcr.read(&img, TextHint::Line).unwrap();
        let text: String = lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join(" ");
        assert!(text.contains("AMMO") && text.contains("24"), "{text:?}");
    }
}
