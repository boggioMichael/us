//! Windows: listing top-level windows and copying their pixels.
//!
//! A window in front is copied from the screen (fast, and what the player
//! sees); a window behind others asks to draw itself (`PrintWindow` with full
//! content, which works for most hardware-accelerated games); a copy that
//! comes back blank is retried the other way. The drawing surface is kept
//! between frames and only rebuilt when the window's size changes.

use std::ffi::c_void;
use std::ptr::null_mut;

use image::RgbaImage;
use syrup_core::Rect;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetClientRect, GetForegroundWindow, GetSystemMetrics, GetWindow,
    GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SM_CXSCREEN,
    SM_CYSCREEN, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use crate::window::WindowInfo;

/// PW_CLIENTONLY | PW_RENDERFULLCONTENT.
const PRINT_CLIENT_FULL: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(0x1 | 0x2);

pub fn set_dpi_aware() {
    // Fails when the process already has an awareness (a manifest): fine either way.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn hwnd(handle: isize) -> HWND {
    HWND(handle as *mut c_void)
}

unsafe extern "system" fn collect(window: HWND, lparam: LPARAM) -> BOOL {
    // Sound: lparam points at the Vec on list_windows' stack for the whole
    // synchronous EnumWindows call.
    let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    out.push(window);
    BOOL(1)
}

fn wide_to_string(buf: &[u16], len: i32) -> String {
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())])
}

fn executable_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..size as usize]))
    }
}

fn client_rect(window: HWND) -> Option<Rect> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(window, &mut rect).ok()?;
        let mut origin = POINT::default();
        if !ClientToScreen(window, &mut origin).as_bool() {
            return None;
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        Some(Rect::new(origin.x, origin.y, w.max(0) as u32, h.max(0) as u32))
    }
}

fn is_cloaked(window: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0
    }
}

/// Visible, titled, top-level windows that could be an application: not
/// tool windows, not owned popups, not hidden by the shell.
pub fn list_windows() -> Vec<WindowInfo> {
    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut Vec<HWND> as isize));
    }
    let foreground = unsafe { GetForegroundWindow() };
    let mut out = Vec::new();
    for window in handles {
        unsafe {
            if !IsWindowVisible(window).as_bool() || is_cloaked(window) {
                continue;
            }
            let ex = GetWindowLongW(window, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 {
                continue;
            }
            if let Ok(owner) = GetWindow(window, GW_OWNER)
                && !owner.is_invalid()
                && IsWindowVisible(owner).as_bool()
            {
                continue;
            }
            let mut title = vec![0u16; 512];
            let n = GetWindowTextW(window, &mut title);
            let title = wide_to_string(&title, n);
            if title.trim().is_empty() {
                continue;
            }
            let mut class = vec![0u16; 256];
            let n = GetClassNameW(window, &mut class);
            let class = wide_to_string(&class, n);
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, Some(&mut pid));
            let path = executable_path(pid);
            let executable = path.as_ref().and_then(|p| p.rsplit(['\\', '/']).next().map(|s| s.to_string()));
            let minimized = IsIconic(window).as_bool();
            let client = client_rect(window).unwrap_or_default();
            out.push(WindowInfo {
                handle: window.0 as isize,
                title,
                class,
                process_id: pid,
                executable,
                executable_path: path,
                client,
                minimized,
                foreground: window == foreground,
            });
        }
    }
    out
}

/// A drawing surface kept between frames.
pub struct Grabber {
    mem: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    size: (i32, i32),
}

// Sound: the memory DC and bitmap belong to the process, not to a thread,
// and a Grabber is only ever used by one thread at a time (it is not Sync).
unsafe impl Send for Grabber {}

impl Grabber {
    pub fn new() -> Self {
        Grabber {
            mem: HDC(null_mut()),
            bitmap: HBITMAP(null_mut()),
            old: HGDIOBJ(null_mut()),
            bits: null_mut(),
            size: (0, 0),
        }
    }

    fn release(&mut self) {
        unsafe {
            if !self.mem.is_invalid() {
                if !self.old.is_invalid() {
                    let _ = SelectObject(self.mem, self.old);
                }
                if !self.bitmap.is_invalid() {
                    let _ = DeleteObject(self.bitmap.into());
                }
                let _ = DeleteDC(self.mem);
            }
        }
        *self = Grabber::new();
    }

    fn ensure(&mut self, w: i32, h: i32) -> bool {
        if self.size == (w, h) && !self.mem.is_invalid() && !self.bits.is_null() {
            return true;
        }
        self.release();
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let _ = ReleaseDC(None, screen);
            if mem.is_invalid() {
                return false;
            }
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
            let mut bits: *mut c_void = null_mut();
            match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) if !bits.is_null() => {
                    self.old = SelectObject(mem, bitmap.into());
                    self.mem = mem;
                    self.bitmap = bitmap;
                    self.bits = bits as *mut u8;
                    self.size = (w, h);
                    true
                }
                Ok(bitmap) => {
                    let _ = DeleteObject(bitmap.into());
                    let _ = DeleteDC(mem);
                    false
                }
                Err(_) => {
                    let _ = DeleteDC(mem);
                    false
                }
            }
        }
    }

    fn pixels(&self) -> &[u8] {
        let (w, h) = self.size;
        // Sound: bits points at the DIB section's w*h*4 bytes while the bitmap lives.
        unsafe { std::slice::from_raw_parts(self.bits, (w as usize) * (h as usize) * 4) }
    }

    /// A surface that is one flat colour: a copy that did not work.
    fn is_blank(&self) -> bool {
        let px = self.pixels();
        let n = px.len() / 4;
        if n == 0 {
            return true;
        }
        let step = (n / 509).max(1);
        let first = &px[0..3];
        !(0..n).step_by(step).any(|i| &px[i * 4..i * 4 + 3] != first)
    }

    fn copy_from_screen(&mut self, x: i32, y: i32) -> bool {
        let (w, h) = self.size;
        unsafe {
            let screen = GetDC(None);
            let ok = BitBlt(self.mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY).is_ok();
            let _ = ReleaseDC(None, screen);
            let _ = GdiFlush();
            ok
        }
    }

    fn print(&mut self, window: HWND) -> bool {
        unsafe {
            let ok = PrintWindow(window, self.mem, PRINT_CLIENT_FULL).as_bool();
            let _ = GdiFlush();
            ok
        }
    }

    fn to_image(&self) -> Option<RgbaImage> {
        let (w, h) = self.size;
        let mut buf = self.pixels().to_vec();
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        RgbaImage::from_raw(w as u32, h as u32, buf)
    }

    /// The window's client area, and where it is on the desktop.
    pub fn grab_window(&mut self, handle: isize) -> Option<(RgbaImage, (i32, i32))> {
        let window = hwnd(handle);
        unsafe {
            if !IsWindow(Some(window)).as_bool() {
                return None;
            }
        }
        let client = client_rect(window)?;
        if client.w == 0 || client.h == 0 || !self.ensure(client.w as i32, client.h as i32) {
            return None;
        }
        // In front: copy it off the screen (fast), else ask it to draw itself;
        // behind other windows, the other way round. A blank result falls back.
        let in_front = unsafe { GetForegroundWindow() } == window;
        let mut ok = false;
        for from_screen in [in_front, !in_front] {
            let got = if from_screen { self.copy_from_screen(client.x, client.y) } else { self.print(window) };
            if got && !self.is_blank() {
                ok = true;
                break;
            }
        }
        if !ok {
            return None;
        }
        Some((self.to_image()?, (client.x, client.y)))
    }

    /// The primary screen.
    pub fn grab_screen(&mut self) -> Option<(RgbaImage, (i32, i32))> {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        if w <= 0 || h <= 0 || !self.ensure(w, h) || !self.copy_from_screen(0, 0) {
            return None;
        }
        Some((self.to_image()?, (0, 0)))
    }
}

impl Default for Grabber {
    fn default() -> Self {
        Grabber::new()
    }
}

impl Drop for Grabber {
    fn drop(&mut self) {
        self.release();
    }
}
