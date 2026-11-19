#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod hook;
mod keys;
mod llm;
mod pill;
mod romaji;
mod settings;

use std::io::Write;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use settings::Settings;

/// Timings and outcomes go to %LOCALAPPDATA%\TypeBoost\spike.log.
/// Typed text is only written when TYPEBOOST_DEBUG is set.
pub fn log(message: &str) {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = std::path::Path::new(&base).join("TypeBoost");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("spike.log")) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(file, "[{secs}] {message}");
    }
}

pub fn debug() -> bool {
    std::env::var("TYPEBOOST_DEBUG").is_ok()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    api_key_set: bool,
    workspace_env_set: bool,
    config_path: String,
    mock: bool,
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings.save()?;
    hook::set_trigger(&settings.trigger);
    Ok(())
}

#[tauri::command]
fn get_status() -> Status {
    Status {
        api_key_set: llm::api_key().is_some(),
        workspace_env_set: std::env::var("ANTHROPIC_WORKSPACE_ID").is_ok_and(|w| !w.trim().is_empty()),
        config_path: Settings::path().display().to_string(),
        mock: llm::provider() == "mock",
    }
}

#[tauri::command]
fn list_apps() -> Vec<String> {
    keys::visible_apps()
}

/// Try the given (possibly unsaved) settings on a sample, without touching any document.
#[tauri::command(async)]
fn preview(settings: Settings, input: String, app: Option<String>) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("試す文章を入力してください".into());
    }
    let (tone, _) = settings.tone_for(app.as_deref());
    let has_kanji = input.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
    let mode = if has_kanji || input.contains('\n') { llm::Mode::Selection } else { llm::Mode::Kana };
    tauri::async_runtime::block_on(llm::polish(&settings, &tone, mode, input, &mut |_| {}, &|| false))
}

fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("TypeBoost 設定")
        .inner_size(860.0, 700.0)
        .min_inner_size(640.0, 480.0)
        .build();
    if let Err(e) = built {
        log(&format!("could not open settings window: {e}"));
    }
}

/// Two instances would mean two keyboard hooks fighting over every key.
fn already_running() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\TypeBoost.SingleInstance\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() == ERROR_ALREADY_EXISTS
    }
}

fn main() {
    if already_running() {
        return;
    }
    let show_settings = std::env::args().any(|a| a == "--settings");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, get_status, list_apps, preview])
        .setup(move |app| {
            let handle = app.handle().clone();
            pill::init(&handle);

            let open = MenuItem::with_id(app, "settings", "設定…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "TypeBoost を終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("TypeBoost")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => open_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let settings = Settings::load();
            log(&format!(
                "started: provider={} model={} dict={} app_rules={}",
                llm::provider(),
                settings.model,
                settings.dictionary.len(),
                settings.app_rules.len()
            ));
            hook::set_trigger(&settings.trigger);

            let (tx, rx) = mpsc::channel();
            hook::start(tx);
            let worker = handle.clone();
            std::thread::spawn(move || {
                for job in rx {
                    engine::handle(&worker, job);
                }
            });

            if show_settings {
                open_settings(&handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run TypeBoost");
}
