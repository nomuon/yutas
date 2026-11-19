//! Synthetic key input, clipboard round-trips and IME state (Windows).

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Marks input we injected as part of an operation: the hook ignores it.
pub const MAGIC: usize = 0x5459_4231;
/// Marks replayed type-ahead: the hook tracks it but never queues it again.
pub const MAGIC_REPLAY: usize = 0x5459_4232;

pub struct QKey {
    pub vk: u16,
    pub scan: u16,
    pub ext: bool,
    pub up: bool,
}

fn is_ext(vk: u16) -> bool {
    matches!(
        vk,
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_INSERT | VK_DELETE
    )
}

fn input(vk: u16, scan: u16, up: bool, ext: bool, extra: usize) -> INPUT {
    let mut flags = 0;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if ext {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: extra },
        },
    }
}

fn key(vk: u16, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    input(vk, scan, up, is_ext(vk), MAGIC)
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
    }
}

pub fn tap(vk: u16) {
    send(&[key(vk, false), key(vk, true)]);
}

fn chord(modifier: u16, vk: u16) {
    send(&[key(modifier, false), key(vk, false), key(vk, true), key(modifier, true)]);
}

/// Extend the selection `n` characters to the left of the caret.
pub fn select_left(n: usize) {
    let mut inputs = Vec::with_capacity(n * 2 + 2);
    inputs.push(key(VK_SHIFT, false));
    for _ in 0..n {
        inputs.push(key(VK_LEFT, false));
        inputs.push(key(VK_LEFT, true));
    }
    inputs.push(key(VK_SHIFT, true));
    send(&inputs);
}

/// Replay keys that were held back while an operation was running.
pub fn replay(keys: &[QKey]) {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|k| input(k.vk, k.scan, k.up, k.ext, MAGIC_REPLAY))
        .collect();
    send(&inputs);
}

pub fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

pub fn set_clipboard_text(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .is_ok()
}

/// Ctrl+C the current selection and return it, or None if nothing was copied.
pub fn copy_selection() -> Option<String> {
    let before = unsafe { GetClipboardSequenceNumber() };
    chord(VK_CONTROL, b'C' as u16);
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        sleep(Duration::from_millis(10));
        if unsafe { GetClipboardSequenceNumber() } != before {
            sleep(Duration::from_millis(15));
            return clipboard_text();
        }
    }
    None
}

/// Put `text` on the clipboard and Ctrl+V it over the current selection.
pub fn paste(text: &str) -> bool {
    if !set_clipboard_text(text) {
        return false;
    }
    sleep(Duration::from_millis(30));
    chord(VK_CONTROL, b'V' as u16);
    sleep(Duration::from_millis(180));
    true
}

/// True when the foreground window is a terminal, where Ctrl+C interrupts
/// the running program instead of copying.
pub fn foreground_is_terminal() -> bool {
    let mut buf = [0u16; 128];
    let len = unsafe { GetClassNameW(GetForegroundWindow(), buf.as_mut_ptr(), buf.len() as i32) };
    let class = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
    matches!(class.as_str(), "ConsoleWindowClass" | "CASCADIA_HOSTING_WINDOW_CLASS" | "mintty" | "PuTTY")
}

fn exe_of(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_lowercase)
    }
}

/// Executable name of the app the user is typing in, e.g. "slack.exe".
pub fn foreground_exe() -> Option<String> {
    exe_of(unsafe { GetForegroundWindow() })
}

/// Executable names of the apps that currently have a visible window.
pub fn visible_apps() -> Vec<String> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = &mut *(lparam as *mut Vec<String>);
        if IsWindowVisible(hwnd) != 0 && GetWindowTextLengthW(hwnd) > 0 {
            if let Some(exe) = exe_of(hwnd) {
                found.push(exe);
            }
        }
        1
    }
    let mut found: Vec<String> = Vec::new();
    unsafe {
        EnumWindows(Some(each), &mut found as *mut Vec<String> as LPARAM);
    }
    found.retain(|exe| exe != "typeboost.exe");
    found.sort();
    found.dedup();
    found
}

/// Whether the IME of the focused control is on. None when it cannot be told.
pub fn ime_open() -> Option<bool> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        let tid = GetWindowThreadProcessId(fg, null_mut());
        let mut gi: GUITHREADINFO = zeroed();
        gi.cbSize = size_of::<GUITHREADINFO>() as u32;
        let target = if GetGUIThreadInfo(tid, &mut gi) != 0 && !gi.hwndFocus.is_null() {
            gi.hwndFocus
        } else {
            fg
        };
        let ime = ImmGetDefaultIMEWnd(target);
        if ime.is_null() {
            return None;
        }
        const IMC_GETOPENSTATUS: usize = 0x0005;
        let mut result: usize = 0;
        let ok = SendMessageTimeoutW(
            ime,
            WM_IME_CONTROL,
            IMC_GETOPENSTATUS,
            0,
            SMTO_ABORTIFHUNG,
            100,
            &mut result,
        );
        if ok == 0 {
            None
        } else {
            Some(result != 0)
        }
    }
}
