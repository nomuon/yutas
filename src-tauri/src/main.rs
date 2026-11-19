#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod engine;
mod hook;
mod keys;
mod llm;
mod pill;
mod romaji;
mod secrets;
mod settings;

use std::io::Write;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use settings::Settings;

/// Timings and outcomes go to %LOCALAPPDATA%\yutas\yutas.log.
/// Typed text is only written when YUTAS_DEBUG is set.
pub fn log(message: &str) {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = std::path::Path::new(&base).join("yutas");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("yutas.log")) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(file, "[{secs}] {message}");
    }
}

pub fn debug() -> bool {
    std::env::var("YUTAS_DEBUG").is_ok()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    /// "stored" (credential manager), "env" or "none"
    key_source: &'static str,
    autostart: bool,
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
        key_source: secrets::source(),
        autostart: autostart::is_enabled(),
        config_path: Settings::path().display().to_string(),
        mock: std::env::var("YUTAS_PROVIDER").is_ok_and(|v| v.trim() == "mock"),
    }
}

#[tauri::command]
fn set_api_key(key: String) -> Result<(), String> {
    secrets::store(&key)
}

#[tauri::command]
fn delete_api_key() -> Result<(), String> {
    secrets::delete()
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    autostart::set(enabled)
}

#[tauri::command]
fn list_apps() -> Vec<String> {
    keys::visible_apps()
}

#[tauri::command(async)]
fn list_local_models(url: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::block_on(llm::local_models(&url))
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

/// `tab` opens the window on that tab: "tone", "dict" or "general".
fn open_settings(app: &AppHandle, tab: Option<&str>) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let page = match tab {
        Some(tab) if matches!(tab, "tone" | "dict" | "general") => format!("settings.html#{tab}"),
        _ => "settings.html".to_string(),
    };
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(page.into()))
        .title("yutas 設定")
        .inner_size(860.0, 720.0)
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
    let name: Vec<u16> = "Local\\yutas.SingleInstance\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() == ERROR_ALREADY_EXISTS
    }
}

fn main() {
    if already_running() {
        return;
    }
    // --settings opens the settings window at startup; --settings=general picks a tab.
    let settings_arg = std::env::args().find(|a| a == "--settings" || a.starts_with("--settings="));

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_status,
            set_api_key,
            delete_api_key,
            set_autostart,
            list_apps,
            list_local_models,
            preview
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            pill::init(&handle);

            let open = MenuItem::with_id(app, "settings", "設定…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "yutas を終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("yutas")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => open_settings(app, None),
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let settings = Settings::load();
            log(&format!(
                "started: provider={} model={} key={} dict={} app_rules={}",
                llm::provider(&settings),
                settings.active_model(),
                secrets::source(),
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

            if let Some(arg) = &settings_arg {
                open_settings(&handle, arg.strip_prefix("--settings="));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run yutas");
}
