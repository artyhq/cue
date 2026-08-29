mod audio;
mod dsp;
mod settings;
mod tray;
mod vosk_recognizer;
#[cfg(windows)]
mod win_capture;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct LastToggle(Mutex<Option<Instant>>);
struct Arming(AtomicBool);

fn is_arming(app: &tauri::AppHandle) -> bool {
    app.state::<Arming>().0.load(Ordering::SeqCst)
}

fn set_arming(app: &tauri::AppHandle, value: bool) {
    app.state::<Arming>().0.store(value, Ordering::SeqCst);
}

pub(crate) fn focus_overlay(app: &tauri::AppHandle) {
    if !settings::wants_pill(app) {
        return;
    }
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let _ = window.set_focus();
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        if let Ok(raw) = window.hwnd() {
            let hwnd = windows::Win32::Foundation::HWND(raw.0 as *mut _);
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

#[tauri::command]
fn start_recording_cmd(app: tauri::AppHandle) -> Result<(), String> {
    if audio::is_recording(&app) {
        return Ok(());
    }
    if !is_arming(&app) {
        return Ok(());
    }
    set_arming(&app, false);
    match audio::start_recording(app.clone()) {
        Ok(()) => {
            tray::set_state(&app, tray::TrayState::Recording);
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to start recording: {e}");
            tray::set_state(&app, tray::TrayState::Error);
            let _ = app.emit("cue://error", e.clone());
            show_overlay(&app, false);
            focus_overlay(&app);
            Err(e)
        }
    }
}

#[tauri::command]
fn stop_recording_cmd(app: tauri::AppHandle) {
    set_arming(&app, false);
    focus_overlay(&app);
    audio::stop_recording(app, false, false);
}

#[tauri::command]
fn cancel_recording_cmd(app: tauri::AppHandle) {
    set_arming(&app, false);
    tray::set_state(&app, tray::TrayState::Idle);
    audio::stop_recording(app, true, false);
}

pub(crate) fn show_overlay(app: &tauri::AppHandle, announce: bool) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    if !settings::wants_pill(app) {
        if announce {
            let _ = window.emit("overlay-shown", ());
        }
        return;
    }
    if let Ok(Some(monitor)) = window.primary_monitor() {
        if let Ok(size) = window.outer_size() {
            let monitor_pos = monitor.position();
            let monitor_size = monitor.size();
            let x = monitor_pos.x + (monitor_size.width as i32 - size.width as i32) / 2;
            let y = monitor_pos.y + 72;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }

    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOPMOST,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
        };
        let previous = unsafe { GetForegroundWindow() };
        let _ = window.show();
        if let Ok(raw) = window.hwnd() {
            let hwnd = windows::Win32::Foundation::HWND(raw.0 as *mut _);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                if previous != hwnd && !previous.is_invalid() {
                    let _ = SetForegroundWindow(previous);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window.show();
    }

    if announce {
        let _ = window.emit("overlay-shown", ());
    }
}

fn show_settings(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
        settings::apply_theme(app, &settings::current(app).theme);
    }
}

fn toggle_overlay(app: &tauri::AppHandle) {
    if audio::is_recording(app) {
        set_arming(app, false);
        focus_overlay(app);
        audio::stop_recording(app.clone(), false, false);
        return;
    }
    if is_arming(app) {
        set_arming(app, false);
        tray::set_state(app, tray::TrayState::Idle);
        audio::stop_recording(app.clone(), true, false);
        return;
    }

    set_arming(app, true);
    tray::set_state(app, tray::TrayState::Recording);
    show_overlay(app, true);
}

fn should_toggle(app: &tauri::AppHandle) -> bool {
    if audio::is_recording(app) || is_arming(app) {
        return true;
    }
    let last = app.state::<LastToggle>();
    let mut guard = last.0.lock().unwrap();
    if let Some(prev) = *guard {
        if prev.elapsed() < Duration::from_millis(400) {
            return false;
        }
    }
    *guard = Some(Instant::now());
    true
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let current = settings::current(app).shortcut;
                    let Ok(record) = current.parse::<Shortcut>() else {
                        return;
                    };
                    if *shortcut != record {
                        return;
                    }
                    if should_toggle(app) {
                        toggle_overlay(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let loaded = settings::load();
            settings::apply_theme(app.handle(), &loaded.theme);
            settings::apply_runtime(app.handle(), &loaded);
            app.manage(settings::SettingsState(Mutex::new(loaded.clone())));
            app.manage(audio::AudioState(Mutex::new(None)));
            app.manage(audio::MonitorState(Mutex::new(None)));
            app.manage(LastToggle(Mutex::new(None)));
            app.manage(Arming(AtomicBool::new(false)));
            
            vosk_recognizer::init(app.handle());
            vosk_recognizer::update(app.handle());

            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Record / Stop", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show_i, &settings_i, &sep, &quit_i])?;

            TrayIconBuilder::with_id(tray::TRAY_ID)
                .icon(tray::idle_icon())
                .tooltip(tray::idle_tooltip())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        toggle_overlay(app);
                    }
                    "settings" => {
                        show_settings(app);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_overlay(tray.app_handle());
                    }
                })
                .build(app)?;

            let shortcut: Shortcut = loaded
                .shortcut
                .parse()
                .or_else(|_| settings::default_shortcut().parse())?;
            app.global_shortcut().register(shortcut)?;

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                match window.label() {
                    "overlay" => {
                        set_arming(window.app_handle(), false);
                        let _ = window.hide();
                        let _ = window.emit("overlay-hidden", ());
                        audio::stop_recording(window.app_handle().clone(), true, false);
                        api.prevent_close();
                    }
                    "settings" => {
                        audio::stop_monitor(window.app_handle());
                        let _ = window.hide();
                        api.prevent_close();
                    }
                    _ => {}
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            start_recording_cmd,
            stop_recording_cmd,
            cancel_recording_cmd,
            settings::get_settings,
            settings::update_settings,
            settings::pick_output_dir,
            settings::open_output_dir,
            settings::reveal_saved,
            settings::open_saved,
            audio::list_audio_devices,
            audio::list_playing_apps,
            audio::start_input_monitor,
            audio::stop_input_monitor,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
