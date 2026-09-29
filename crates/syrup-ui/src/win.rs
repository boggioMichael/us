//! Windows: the overlay window and the system voice.
//!
//! The overlay is a layered, top-most tool window drawn with per-pixel
//! alpha: fully transparent pixels let every click through to the game, the
//! card takes its own clicks (feedback buttons), and it never takes the
//! focus (`WS_EX_NOACTIVATE`, `MA_NOACTIVATE`). It asks to be left out of
//! screen captures (`WDA_EXCLUDEFROMCAPTURE`), so Syrup never reads its own
//! card back. The voice is SAPI's default voice on a thread of its own.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
};
use windows::Win32::Media::Speech::{ISpVoice, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SpVoice};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetSystemMetrics, IDC_ARROW, LoadCursorW, MA_NOACTIVATE, MSG,
    PM_REMOVE, PeekMessageW, RegisterClassExW, SM_CXSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SetWindowDisplayAffinity,
    ShowWindow, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::{Painted, UiAction};

/// Clicks on the overlay, collected by the window procedure.
static CLICKS: Mutex<Vec<(isize, i32, i32)>> = Mutex::new(Vec::new());

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            if let Ok(mut c) = CLICKS.lock() {
                c.push((hwnd.0 as isize, x, y));
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub struct OverlayWindow {
    hwnd: HWND,
    painted: Option<Painted>,
    at: (i32, i32),
    visible: bool,
    /// Whether Windows agreed to keep it out of screen captures (Windows 10 2004 and later).
    pub hidden_from_capture: bool,
}

impl OverlayWindow {
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(PCWSTR::null())?.into();
            let class = w!("SyrupUniversalOverlay");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Syrup"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )?;
            let hidden_from_capture = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_ok();
            Ok(OverlayWindow { hwnd, painted: None, at: (0, 0), visible: false, hidden_from_capture })
        }
    }

    /// Where to put a picture of `w` pixels wide: the top right of the game
    /// window's client area (or of the screen).
    pub fn place(anchor: Option<syrup_core::Rect>, w: u32) -> (i32, i32) {
        match anchor {
            Some(r) => (r.right() - w as i32 - 12, r.y + 12),
            None => {
                let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) };
                (sw - w as i32 - 16, 16)
            }
        }
    }

    /// Handles what Windows sent the window (call often, from the thread that created it).
    pub fn pump(&self) {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(self.hwnd), 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }
    }

    /// What the player clicked since the last call.
    pub fn take_actions(&mut self) -> Vec<UiAction> {
        let clicks: Vec<(isize, i32, i32)> = match CLICKS.lock() {
            Ok(mut c) => std::mem::take(&mut *c),
            Err(_) => Vec::new(),
        };
        let Some(p) = &self.painted else { return Vec::new() };
        clicks
            .into_iter()
            .filter(|(h, _, _)| *h == self.hwnd.0 as isize)
            .filter_map(|(_, x, y)| p.action_at(x, y))
            .collect()
    }

    pub fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }

    /// Shows `painted` with its top left at `at` (desktop pixels).
    pub fn show(&mut self, painted: Painted, at: (i32, i32)) -> windows::core::Result<()> {
        let (w, h) = (painted.image.width() as i32, painted.image.height() as i32);
        if w <= 1 || h <= 1 {
            self.hide();
            self.painted = Some(painted);
            return Ok(());
        }
        if self.visible
            && self.at == at
            && self.painted.as_ref().is_some_and(|p| p.image.as_raw() == painted.image.as_raw())
        {
            return Ok(());
        }
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(d) if !bits.is_null() => d,
                Ok(d) => {
                    let _ = DeleteObject(d.into());
                    let _ = DeleteDC(mem);
                    let _ = ReleaseDC(None, screen);
                    return Err(windows::core::Error::from_win32());
                }
                Err(e) => {
                    let _ = DeleteDC(mem);
                    let _ = ReleaseDC(None, screen);
                    return Err(e);
                }
            };
            let old = SelectObject(mem, dib.into());
            // Premultiplied BGRA, as layered windows want it.
            let buf = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            for (i, p) in painted.image.pixels().enumerate() {
                let a = p.0[3] as u32;
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                buf[4 * i..4 * i + 4].copy_from_slice(&[pm(p.0[2]), pm(p.0[1]), pm(p.0[0]), p.0[3]]);
            }
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let dst = POINT { x: at.0, y: at.1 };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let done = UpdateLayeredWindow(
                self.hwnd,
                Some(screen),
                Some(&dst),
                Some(&size),
                Some(mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = SelectObject(mem, old);
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            done?;
            if !self.visible {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.visible = true;
            }
        }
        self.at = at;
        self.painted = Some(painted);
        Ok(())
    }
}

/// The system's default voice, on a thread of its own; each line cuts off the last.
pub struct Speech {
    lines: Sender<String>,
}

impl Speech {
    /// `rate` from -10 (slow) to 10 (fast); `None` when there is no voice.
    pub fn start(rate: i32) -> Option<Speech> {
        let (lines, rx) = channel::<String>();
        let (ready, is_ready) = channel::<bool>();
        std::thread::Builder::new()
            .name("syrup-voice".into())
            .spawn(move || {
                let voice: ISpVoice = unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    match CoCreateInstance(&SpVoice, None, CLSCTX_ALL) {
                        Ok(v) => v,
                        Err(_) => {
                            let _ = ready.send(false);
                            return;
                        }
                    }
                };
                unsafe {
                    let _ = voice.SetRate(rate);
                }
                let _ = ready.send(true);
                let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
                // The text being spoken is kept alive until the next line replaces it.
                let mut speaking: Vec<u16> = Vec::new();
                for line in rx {
                    let text: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
                    unsafe {
                        let _ = voice.Speak(PCWSTR(text.as_ptr()), flags, None);
                    }
                    speaking = text;
                }
                unsafe {
                    let _ = voice.WaitUntilDone(10_000);
                }
                drop(speaking);
            })
            .ok()?;
        is_ready.recv().ok().filter(|ok| *ok).map(|_| Speech { lines })
    }

    pub fn say(&self, line: &str) {
        let _ = self.lines.send(line.to_string());
    }
}
