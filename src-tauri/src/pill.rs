//! The floating status pill. The window is always present, transparent,
//! click-through and never activated; the page fades the pill in and out.

use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

static HWND_PILL: AtomicIsize = AtomicIsize::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Clone)]
struct Payload<'a> {
    state: &'a str,
    text: &'a str,
    hint: &'a str,
}

pub fn init(app: &AppHandle) {
    let Some(window) = app.get_webview_window("pill") else { return };
    let _ = window.set_ignore_cursor_events(true);
    if let Ok(hwnd) = window.hwnd() {
        let hwnd = hwnd.0 as HWND;
        HWND_PILL.store(hwnd as isize, Ordering::SeqCst);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize);
        }
    }
    place();
}

/// Bottom centre of the monitor the user is typing on.
fn place() {
    let hwnd = HWND_PILL.load(Ordering::SeqCst) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        let mut rect: RECT = zeroed();
        if GetMonitorInfoW(monitor, &mut info) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
            return;
        }
        let work = info.rcWork;
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.bottom - h - 40;
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

fn emit(app: &AppHandle, state: &str, text: &str, hint: &str) {
    let _ = app.emit("pill", Payload { state, text, hint });
}

/// Show a state that stays until replaced.
pub fn show(app: &AppHandle, state: &str, text: &str, hint: &str) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    place();
    emit(app, state, text, hint);
}

/// Update the text of the current state without moving the window.
pub fn update(app: &AppHandle, state: &str, text: &str, hint: &str) {
    emit(app, state, text, hint);
}

/// Show a final state, then fade out.
pub fn flash(app: &AppHandle, state: &str, text: &str, hint: &str, ms: u64) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    place();
    emit(app, state, text, hint);
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(ms));
        if GENERATION.load(Ordering::SeqCst) == generation {
            emit(&app, "hide", "", "");
        }
    });
}
