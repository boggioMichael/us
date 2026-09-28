//! Game windows: finding them, picking the right one, and capturing it.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
#[cfg(windows)]
use syrup_core::Frame;
use syrup_core::Rect;
use syrup_core::frame::{SourceInfo, SourceKind};

use crate::{Capture, CaptureError, FrameSource};

/// A top-level window on the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// The window handle, as a number.
    pub handle: isize,
    pub title: String,
    pub class: String,
    pub process_id: u32,
    /// The executable's file name (`eldenring.exe`).
    pub executable: Option<String>,
    pub executable_path: Option<String>,
    /// The client area (what the game draws), in desktop pixels.
    pub client: Rect,
    pub minimized: bool,
    pub foreground: bool,
}

impl WindowInfo {
    pub fn source_info(&self) -> SourceInfo {
        SourceInfo {
            kind: SourceKind::Window,
            window_title: Some(self.title.clone()),
            executable: self.executable.clone(),
            executable_path: self.executable_path.clone(),
            process_id: Some(self.process_id),
            window_class: Some(self.class.clone()),
            path: None,
        }
    }
}

/// Which window to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowSelector {
    /// The first window whose title contains this (any case).
    Title(String),
    /// The window of this executable (`MapleStory.exe`, any case, `.exe` optional).
    Executable(String),
    Handle(isize),
    /// Whatever game window is in front; follows the player when they switch games.
    Auto,
}

impl WindowSelector {
    pub fn describe(&self) -> String {
        match self {
            WindowSelector::Title(t) => format!("the window titled \"{t}\""),
            WindowSelector::Executable(e) => format!("the window of {e}"),
            WindowSelector::Handle(h) => format!("window {h:#x}"),
            WindowSelector::Auto => "the game in front".to_string(),
        }
    }
}

/// Programs whose windows are never the game: the desktop and taskbar,
/// terminals, launchers and chat apps, capture tools, and Syrup itself.
const NOT_GAMES: &[&str] = &[
    "explorer.exe",
    "searchhost.exe",
    "searchapp.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "textinputhost.exe",
    "lockapp.exe",
    "systemsettings.exe",
    "taskmgr.exe",
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "code.exe",
    "devenv.exe",
    "claude.exe",
    "syrup.exe",
    "obs64.exe",
    "discord.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "battle.net.exe",
    "riotclientux.exe",
    "nvidia overlay.exe",
    "gamebar.exe",
];

const NOT_GAME_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "ConsoleWindowClass",
    "CASCADIA_HOSTING_WINDOW_CLASS",
    "SyrupUniversalOverlay",
];

/// Is this window certainly not a game (or too small to be one)?
pub fn is_excluded(w: &WindowInfo, own_pid: u32) -> bool {
    if w.process_id == own_pid || w.title.trim().is_empty() {
        return true;
    }
    if NOT_GAME_CLASSES.iter().any(|c| w.class == *c) {
        return true;
    }
    if let Some(exe) = &w.executable {
        let exe = exe.to_ascii_lowercase();
        if NOT_GAMES.contains(&exe.as_str()) {
            return true;
        }
    }
    !w.minimized && (w.client.w < 160 || w.client.h < 120)
}

fn exe_matches(w: &WindowInfo, wanted: &str) -> bool {
    let wanted = wanted.to_ascii_lowercase();
    let wanted = wanted.strip_suffix(".exe").unwrap_or(&wanted);
    w.executable.as_ref().is_some_and(|e| {
        let e = e.to_ascii_lowercase();
        e.strip_suffix(".exe").unwrap_or(&e) == wanted
    })
}

/// The window the selector means, among `windows`.
pub fn pick_window(
    windows: &[WindowInfo],
    selector: &WindowSelector,
    own_pid: u32,
) -> Option<WindowInfo> {
    match selector {
        WindowSelector::Title(t) => {
            let t = t.to_lowercase();
            windows
                .iter()
                .filter(|w| w.process_id != own_pid && w.title.to_lowercase().contains(&t))
                .max_by_key(|w| (w.foreground, w.client.area()))
                .cloned()
        }
        WindowSelector::Executable(e) => windows
            .iter()
            .filter(|w| exe_matches(w, e))
            .max_by_key(|w| (w.foreground, w.client.area()))
            .cloned(),
        WindowSelector::Handle(h) => windows.iter().find(|w| w.handle == *h).cloned(),
        WindowSelector::Auto => {
            if let Some(front) = windows
                .iter()
                .find(|w| w.foreground && !is_excluded(w, own_pid) && !w.minimized)
            {
                return Some(front.clone());
            }
            windows
                .iter()
                .filter(|w| !is_excluded(w, own_pid) && !w.minimized)
                .max_by_key(|w| w.client.area())
                .cloned()
        }
    }
}

/// Every visible top-level window with a title (Windows; empty elsewhere).
pub fn list_windows() -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        crate::win32::list_windows()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// A game window, captured live.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct WindowSource {
    selector: WindowSelector,
    current: Option<WindowInfo>,
    info: Arc<SourceInfo>,
    started: Instant,
    index: u64,
    fps: f32,
    last_resolve: Option<Instant>,
    /// Auto mode: a different window in front, and since when (switching waits a moment).
    challenger: Option<(isize, Instant)>,
    own_pid: u32,
    #[cfg(windows)]
    grabber: crate::win32::Grabber,
}

impl WindowSource {
    pub fn new(selector: WindowSelector, fps: f32) -> Result<Self, CaptureError> {
        #[cfg(not(windows))]
        {
            let _ = (selector, fps);
            Err(CaptureError::Unsupported(
                "live window capture needs Windows; use a recording instead".into(),
            ))
        }
        #[cfg(windows)]
        {
            let mut s = WindowSource {
                selector,
                current: None,
                info: Arc::new(SourceInfo::new(SourceKind::Window)),
                started: Instant::now(),
                index: 0,
                fps: fps.clamp(0.5, 60.0),
                last_resolve: None,
                challenger: None,
                own_pid: std::process::id(),
                grabber: crate::win32::Grabber::new(),
            };
            s.resolve();
            if s.current.is_none() && !matches!(s.selector, WindowSelector::Auto) {
                return Err(CaptureError::NotFound(s.selector.describe()));
            }
            Ok(s)
        }
    }

    /// The window being watched now.
    pub fn window(&self) -> Option<&WindowInfo> {
        self.current.as_ref()
    }

    /// Finds the window again (it may have moved, been renamed, closed, or the
    /// player may have switched to another game).
    fn resolve(&mut self) {
        let now = Instant::now();
        self.last_resolve = Some(now);
        let windows = list_windows();
        let still_there = self
            .current
            .as_ref()
            .and_then(|c| windows.iter().find(|w| w.handle == c.handle).cloned());
        let picked = pick_window(&windows, &self.selector, self.own_pid);
        let next = match (&self.selector, still_there, picked) {
            (WindowSelector::Auto, Some(current), Some(front))
                if front.handle != current.handle =>
            {
                // Switch only when the other window has stayed in front for a while.
                match self.challenger {
                    Some((h, since))
                        if h == front.handle && now.duration_since(since).as_secs_f32() >= 2.0 =>
                    {
                        self.challenger = None;
                        Some(front)
                    }
                    Some((h, _)) if h == front.handle => Some(current),
                    _ => {
                        self.challenger = Some((front.handle, now));
                        Some(current)
                    }
                }
            }
            (_, Some(current), _) => {
                self.challenger = None;
                Some(current)
            }
            (_, None, picked) => picked,
        };
        let changed = next.as_ref().map(|w| (w.handle, &w.title))
            != self.current.as_ref().map(|w| (w.handle, &w.title));
        if changed {
            self.info = Arc::new(
                next.as_ref()
                    .map(|w| w.source_info())
                    .unwrap_or_else(|| SourceInfo::new(SourceKind::Window)),
            );
        }
        self.current = next;
    }
}

impl FrameSource for WindowSource {
    fn info(&self) -> Arc<SourceInfo> {
        self.info.clone()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        let due = self
            .last_resolve
            .is_none_or(|t| t.elapsed().as_millis() >= 1000);
        if due || self.current.is_none() {
            self.resolve();
        }
        let Some(window) = self.current.clone() else {
            return Ok(Capture::Waiting(format!(
                "looking for {}",
                self.selector.describe()
            )));
        };
        if window.minimized {
            return Ok(Capture::Waiting(format!("{} is minimised", window.title)));
        }
        #[cfg(windows)]
        {
            match self.grabber.grab_window(window.handle) {
                Some((image, origin)) => {
                    let ts = self.started.elapsed().as_millis() as u64;
                    let mut frame = Frame::new(self.index, ts, image, self.info.clone());
                    frame.origin = origin;
                    self.index += 1;
                    Ok(Capture::Frame(frame))
                }
                None => {
                    self.current = None;
                    Ok(Capture::Waiting(format!(
                        "could not capture {}",
                        window.title
                    )))
                }
            }
        }
        #[cfg(not(windows))]
        {
            Err(CaptureError::Unsupported(
                "live window capture needs Windows".into(),
            ))
        }
    }

    fn nominal_fps(&self) -> f32 {
        self.fps
    }
}

/// The whole primary screen, captured live.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct ScreenSource {
    info: Arc<SourceInfo>,
    started: Instant,
    index: u64,
    fps: f32,
    #[cfg(windows)]
    grabber: crate::win32::Grabber,
}

impl ScreenSource {
    pub fn new(fps: f32) -> Result<Self, CaptureError> {
        #[cfg(not(windows))]
        {
            let _ = fps;
            Err(CaptureError::Unsupported(
                "live screen capture needs Windows; use a recording instead".into(),
            ))
        }
        #[cfg(windows)]
        {
            let info = SourceInfo::new(SourceKind::Screen).with_title("Screen");
            Ok(ScreenSource {
                info: Arc::new(info),
                started: Instant::now(),
                index: 0,
                fps: fps.clamp(0.5, 60.0),
                grabber: crate::win32::Grabber::new(),
            })
        }
    }
}

impl FrameSource for ScreenSource {
    fn info(&self) -> Arc<SourceInfo> {
        self.info.clone()
    }

    fn next(&mut self) -> Result<Capture, CaptureError> {
        #[cfg(windows)]
        {
            match self.grabber.grab_screen() {
                Some((image, origin)) => {
                    let ts = self.started.elapsed().as_millis() as u64;
                    let mut frame = Frame::new(self.index, ts, image, self.info.clone());
                    frame.origin = origin;
                    self.index += 1;
                    Ok(Capture::Frame(frame))
                }
                None => Ok(Capture::Waiting("the screen could not be captured".into())),
            }
        }
        #[cfg(not(windows))]
        {
            Err(CaptureError::Unsupported(
                "live screen capture needs Windows".into(),
            ))
        }
    }

    fn nominal_fps(&self) -> f32 {
        self.fps
    }
}

/// Makes the process see real pixels on high-DPI displays (call once, first).
pub fn init_process() {
    #[cfg(windows)]
    crate::win32::set_dpi_aware();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(handle: isize, title: &str, exe: &str, fg: bool, w: u32, h: u32) -> WindowInfo {
        WindowInfo {
            handle,
            title: title.into(),
            class: "Game".into(),
            process_id: handle as u32 + 100,
            executable: Some(exe.into()),
            executable_path: Some(format!("C:\\Games\\{exe}")),
            client: Rect::new(0, 0, w, h),
            minimized: false,
            foreground: fg,
        }
    }

    #[test]
    fn auto_picks_the_game_in_front_and_never_the_desktop_or_syrup() {
        let windows = vec![
            win(1, "Program Manager", "explorer.exe", false, 1920, 1080),
            win(2, "Terminal", "WindowsTerminal.exe", true, 900, 500),
            win(3, "ELDEN RING™", "eldenring.exe", false, 1920, 1080),
            win(4, "Discord", "Discord.exe", false, 1200, 800),
        ];
        // The terminal is in front but is not a game: the biggest game-like window wins.
        assert_eq!(
            pick_window(&windows, &WindowSelector::Auto, 0)
                .unwrap()
                .handle,
            3
        );
        let mut w = windows.clone();
        w.push(win(
            5,
            "Hollow Knight",
            "hollow_knight.exe",
            true,
            1280,
            720,
        ));
        w[1].foreground = false;
        assert_eq!(pick_window(&w, &WindowSelector::Auto, 0).unwrap().handle, 5);
        // Syrup's own process is never picked.
        assert!(pick_window(&w, &WindowSelector::Auto, 105).is_some_and(|x| x.handle == 3));
    }

    #[test]
    fn selectors_by_title_and_executable() {
        let windows = vec![
            win(1, "MapleStory", "MapleStory.exe", false, 1366, 768),
            win(2, "Notes", "notepad.exe", true, 800, 600),
        ];
        assert_eq!(
            pick_window(&windows, &WindowSelector::Title("maple".into()), 0)
                .unwrap()
                .handle,
            1
        );
        assert_eq!(
            pick_window(
                &windows,
                &WindowSelector::Executable("maplestory".into()),
                0
            )
            .unwrap()
            .handle,
            1
        );
        assert_eq!(
            pick_window(
                &windows,
                &WindowSelector::Executable("MAPLESTORY.EXE".into()),
                0
            )
            .unwrap()
            .handle,
            1
        );
        assert!(pick_window(&windows, &WindowSelector::Title("zelda".into()), 0).is_none());
        assert!(is_excluded(&win(9, "tiny", "game.exe", false, 100, 80), 0));
    }
}
