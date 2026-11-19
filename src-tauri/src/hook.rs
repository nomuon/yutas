//! Low-level keyboard/mouse hooks: double-tap detection on the trigger key,
//! composition tracking, and holding back type-ahead while an operation runs.

use std::ptr::null_mut;
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::keys::{self, QKey, MAGIC, MAGIC_REPLAY};
use crate::romaji::Composer;

const DOUBLE_TAP: Duration = Duration::from_millis(350);

/// What the user double-taps.
#[derive(Clone, Copy)]
pub enum Trigger {
    /// A modifier (works on any keyboard). It is never held back: a tap is a
    /// press and release with no other key in between.
    Modifier { name: &'static str, vks: &'static [u16] },
    /// A dedicated key such as 変換 (JIS keyboards). It is swallowed, and a
    /// single tap is passed on after the double-tap window.
    Key { name: &'static str, vk: u16 },
}

impl Trigger {
    pub fn name(&self) -> &'static str {
        match self {
            Trigger::Modifier { name, .. } | Trigger::Key { name, .. } => name,
        }
    }
}

impl Trigger {
    /// shift (default) | lshift | rshift | ctrl | lctrl | rctrl | convert
    fn from_setting(choice: &str) -> Trigger {
        match choice.trim().to_lowercase().as_str() {
            "convert" => Trigger::Key { name: "変換", vk: VK_CONVERT },
            "lshift" => Trigger::Modifier { name: "左Shift", vks: &[VK_LSHIFT] },
            "rshift" => Trigger::Modifier { name: "右Shift", vks: &[VK_RSHIFT] },
            "ctrl" => Trigger::Modifier { name: "Ctrl", vks: &[VK_LCONTROL, VK_RCONTROL, VK_CONTROL] },
            "lctrl" => Trigger::Modifier { name: "左Ctrl", vks: &[VK_LCONTROL] },
            "rctrl" => Trigger::Modifier { name: "右Ctrl", vks: &[VK_RCONTROL] },
            _ => Trigger::Modifier { name: "Shift", vks: &[VK_LSHIFT, VK_RSHIFT, VK_SHIFT] },
        }
    }
}

static TRIGGER: LazyLock<RwLock<Trigger>> = LazyLock::new(|| RwLock::new(Trigger::from_setting("shift")));

fn trigger() -> Trigger {
    *TRIGGER.read().unwrap_or_else(|e| e.into_inner())
}

/// Switch the trigger at runtime (called when settings are loaded or saved).
pub fn set_trigger(choice: &str) {
    let new = Trigger::from_setting(choice);
    *TRIGGER.write().unwrap_or_else(|e| e.into_inner()) = new;
    let mut st = state();
    st.tap_pending = false;
    st.tap_time = None;
    st.trigger_down = false;
    crate::log(&format!("trigger: {} x2", new.name()));
}

pub enum Job {
    /// The trigger was double-tapped.
    Trigger,
    /// A single tap of a swallowed trigger key was held back and another key
    /// followed: let it through.
    FlushTap,
}

#[derive(Clone)]
pub struct Last {
    pub original: String,
    pub result: String,
}

#[derive(Default)]
pub struct State {
    pub comp: Composer,
    /// The caret moved or something else happened inside the composition
    /// that we do not mirror; the tracked text can no longer be trusted.
    pub dirty: bool,
    /// The IME is showing conversion candidates.
    pub converting: bool,
    fg: isize,

    /// An operation is running: real keys are queued instead of delivered.
    pub busy: bool,
    pub cancel: bool,
    pub queue: Vec<QKey>,

    /// Time of the first tap, while waiting for the second.
    tap_time: Option<Instant>,
    /// Trigger::Key only: a single tap is being held back.
    tap_pending: bool,
    tap_gen: u64,
    trigger_down: bool,
    /// Trigger::Modifier only: no other key since the modifier went down.
    trigger_clean: bool,

    /// The last result is still right before the caret, untouched.
    pub undo_armed: bool,
    pub last: Option<Last>,
}

impl State {
    pub fn reset_composition(&mut self) {
        self.comp.clear();
        self.dirty = false;
        self.converting = false;
    }

    fn on_convert_key(&mut self) {
        if !self.comp.is_empty() {
            self.converting = true;
        }
    }

    fn within_double_tap(&self) -> bool {
        self.tap_time.is_some_and(|t| t.elapsed() < DOUBLE_TAP)
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
static JOBS: OnceLock<Sender<Job>> = OnceLock::new();

pub fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn send_job(job: Job) {
    if let Some(tx) = JOBS.get() {
        let _ = tx.send(job);
    }
}

pub fn start(jobs: Sender<Job>) {
    let _ = JOBS.set(jobs);
    thread::spawn(|| unsafe {
        let module = GetModuleHandleW(null_mut());
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        crate::log(&format!("hooks installed: keyboard={} mouse={}", !kb.is_null(), !mouse.is_null()));
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {}
    });
}

/// Let a held-back single tap through to the IME (called off the hook thread).
pub fn release_single_tap() {
    if let Trigger::Key { vk, .. } = trigger() {
        if vk == VK_CONVERT {
            state().on_convert_key();
        }
        keys::tap(vk);
    }
}

/// Deliver queued type-ahead, then leave the busy state.
pub fn finish_operation() {
    loop {
        let batch = {
            let mut st = state();
            if st.queue.is_empty() {
                st.busy = false;
                st.cancel = false;
                return;
            }
            std::mem::take(&mut st.queue)
        };
        keys::replay(&batch);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lparam as *const KBDLLHOOKSTRUCT);
        if handle_key(k, wparam as u32) {
            return 1;
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let msg = wparam as u32;
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
            let mut st = state();
            st.reset_composition();
            st.undo_armed = false;
            st.tap_time = None;
            if st.busy {
                st.cancel = true;
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

fn queued(k: &KBDLLHOOKSTRUCT, up: bool) -> QKey {
    QKey {
        vk: k.vkCode as u16,
        scan: k.scanCode as u16,
        ext: k.flags & LLKHF_EXTENDED != 0,
        up,
    }
}

/// Returns true when the key must not reach the application.
fn handle_key(k: &KBDLLHOOKSTRUCT, msg: u32) -> bool {
    if k.dwExtraInfo == MAGIC {
        return false;
    }
    let replay = k.dwExtraInfo == MAGIC_REPLAY;
    let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    let vk = k.vkCode as u16;
    let current = trigger();
    let mut st = state();

    if !replay {
        match current {
            Trigger::Modifier { vks, .. } => {
                if modifier_trigger(&mut st, vks.contains(&vk), up) {
                    st.busy = true;
                    st.cancel = false;
                    drop(st);
                    send_job(Job::Trigger);
                    return false; // the release itself still reaches the app
                }
            }
            Trigger::Key { vk: trigger, .. } if vk == trigger => {
                return key_trigger(st, up);
            }
            Trigger::Key { .. } => {}
        }
    }

    if st.busy && !replay {
        if vk == VK_ESCAPE {
            if !up {
                st.cancel = true;
            }
        } else {
            st.queue.push(queued(k, up));
        }
        return true;
    }

    if up {
        return false;
    }

    if st.tap_pending && !replay && !is_modifier(vk) {
        // A single tap of a swallowed trigger key is still held back; keep
        // the order by queueing this key behind it for the worker to deliver.
        st.tap_pending = false;
        st.tap_gen += 1;
        st.busy = true;
        st.queue.push(queued(k, false));
        drop(st);
        send_job(Job::FlushTap);
        return true;
    }

    track(&mut st, vk);
    false
}

/// Follows a modifier trigger. Returns true on the release that completes a
/// double tap. State is followed even while busy so it never goes stale.
fn modifier_trigger(st: &mut State, is_trigger: bool, up: bool) -> bool {
    if !is_trigger {
        if !up {
            st.trigger_clean = false;
            st.tap_time = None;
        }
        return false;
    }
    if !up {
        if !st.trigger_down {
            st.trigger_down = true;
            st.trigger_clean = true;
        }
        return false;
    }
    st.trigger_down = false;
    if !st.trigger_clean || st.busy {
        st.tap_time = None;
        return false;
    }
    if st.within_double_tap() {
        st.tap_time = None;
        true
    } else {
        st.tap_time = Some(Instant::now());
        false
    }
}

/// Handles a swallowed trigger key. Always returns true (swallow).
fn key_trigger(mut st: MutexGuard<'static, State>, up: bool) -> bool {
    if up {
        st.trigger_down = false;
        return true;
    }
    if st.trigger_down || st.busy {
        st.trigger_down = true; // auto-repeat, or no nested operations
        return true;
    }
    st.trigger_down = true;
    st.tap_gen += 1;
    if st.tap_pending && st.within_double_tap() {
        st.tap_pending = false;
        st.tap_time = None;
        st.busy = true;
        st.cancel = false;
        drop(st);
        send_job(Job::Trigger);
    } else {
        st.tap_pending = true;
        st.tap_time = Some(Instant::now());
        let gen = st.tap_gen;
        drop(st);
        thread::spawn(move || {
            thread::sleep(DOUBLE_TAP);
            let expired = {
                let mut st = state();
                let hit = st.tap_pending && st.tap_gen == gen;
                if hit {
                    st.tap_pending = false;
                }
                hit
            };
            if expired {
                release_single_tap();
            }
        });
    }
    true
}

fn is_modifier(vk: u16) -> bool {
    matches!(
        vk,
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_LSHIFT | VK_RSHIFT | VK_LCONTROL | VK_RCONTROL | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN
    )
}

fn pressed(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

/// Mirror what this key does to the IME composition.
fn track(st: &mut State, vk: u16) {
    if is_modifier(vk) {
        return;
    }
    let fg = unsafe { GetForegroundWindow() } as isize;
    if fg != st.fg {
        st.fg = fg;
        st.reset_composition();
    }
    st.undo_armed = false;

    if pressed(VK_CONTROL) || pressed(VK_MENU) || pressed(VK_LWIN) || pressed(VK_RWIN) {
        st.reset_composition();
        return;
    }

    let composing = !st.comp.is_empty();
    match vk {
        0x41..=0x5A => {
            if st.converting {
                st.reset_composition(); // typing on commits the candidates
            }
            st.comp.push_letter((vk as u8 as char).to_ascii_lowercase(), pressed(VK_SHIFT));
        }
        0x30..=0x39 | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2 => {
            if st.converting {
                st.reset_composition();
            }
            st.comp.push_symbol();
        }
        VK_BACK => {
            if st.converting {
                st.dirty = true;
            } else {
                st.comp.backspace();
                if st.comp.is_empty() {
                    st.reset_composition();
                }
            }
        }
        VK_SPACE | VK_CONVERT => st.on_convert_key(),
        VK_RETURN => st.reset_composition(),
        VK_ESCAPE => {
            if st.converting {
                st.converting = false;
            } else {
                st.reset_composition();
            }
        }
        // IME on/off and mode keys: 半角/全角, 英数, ひらがな, IME on/off
        VK_KANJI | VK_KANA | VK_IME_ON | VK_IME_OFF | 0xF0 | 0xF2 | 0xF3 | 0xF4 => st.reset_composition(),
        _ => {
            if composing {
                st.dirty = true; // arrows, Tab, F6-F10, 無変換, Delete...
            }
        }
    }
}
