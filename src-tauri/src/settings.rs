use std::path::PathBuf;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Theme};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub exe: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub input_device_id: String,
    #[serde(default)]
    pub output_device_id: String,
    #[serde(default)]
    pub output_dir: String,
    #[serde(default)]
    pub active_preset_id: String,
    #[serde(default)]
    pub presets: Vec<Preset>,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    #[serde(default = "default_indicator")]
    pub indicator: String,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub wake_on_voice: bool,
    /// Missing from existing settings.json means the app has already been used.
    #[serde(default = "default_onboarded")]
    pub onboarded: bool,
    #[serde(default)]
    pub onboarding_version: u32,
}

pub const ONBOARDING_VERSION: u32 = 1;

fn default_onboarded() -> bool {
    true
}

pub fn needs_onboarding(settings: &AppSettings) -> bool {
    settings.onboarding_version < ONBOARDING_VERSION
}

fn default_theme() -> String {
    "system".into()
}

pub fn default_shortcut() -> String {
    "ctrl+shift+r".into()
}

fn default_indicator() -> String {
    "pill".into()
}

pub fn default_voice_preset() -> Preset {
    Preset {
        id: "voice".into(),
        name: "Voice".into(),
        channels: vec![
            Channel {
                id: "mic".into(),
                kind: "mic".into(),
                enabled: true,
                name: "My voice".into(),
                exe: String::new(),
            },
            Channel {
                id: "system".into(),
                kind: "system".into(),
                enabled: false,
                name: "Computer sound".into(),
                exe: String::new(),
            },
        ],
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        let voice = default_voice_preset();
        Self {
            theme: default_theme(),
            input_device_id: String::new(),
            output_device_id: String::new(),
            output_dir: String::new(),
            active_preset_id: voice.id.clone(),
            presets: vec![voice],
            shortcut: default_shortcut(),
            indicator: default_indicator(),
            launch_at_login: false,
            wake_on_voice: false,
            onboarded: false,
            onboarding_version: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub theme: String,
    pub input_device_id: String,
    pub output_device_id: String,
    pub output_dir: String,
    pub default_output_dir: String,
    pub active_preset_id: String,
    pub presets: Vec<Preset>,
    pub shortcut: String,
    pub indicator: String,
    pub launch_at_login: bool,
    pub wake_on_voice: bool,
    pub onboarded: bool,
    pub onboarding_version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceDto {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingApp {
    pub exe: String,
    pub name: String,
}

pub struct SettingsState(pub Mutex<AppSettings>);

fn config_path() -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("app", "Cue", "cue").ok_or("No config directory")?;
    let dir = dirs.config_dir();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

pub fn default_output_dir() -> Result<PathBuf, String> {
    let dirs = directories::UserDirs::new().ok_or("No user dirs")?;
    let docs = dirs.document_dir().ok_or("No documents dir")?;
    Ok(docs.join("Cue"))
}

pub fn resolved_output_dir(settings: &AppSettings) -> Result<PathBuf, String> {
    if settings.output_dir.trim().is_empty() {
        default_output_dir()
    } else {
        Ok(PathBuf::from(&settings.output_dir))
    }
}

pub fn load() -> AppSettings {
    let Ok(path) = config_path() else {
        return AppSettings::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return AppSettings::default();
    };
    normalize(serde_json::from_str(&text).unwrap_or_default())
}

fn save(settings: &AppSettings) -> Result<(), String> {
    let path = config_path()?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn current(app: &AppHandle) -> AppSettings {
    app.try_state::<SettingsState>()
        .and_then(|state| state.0.lock().ok().map(|g| g.clone()))
        .unwrap_or_default()
}

pub fn active_preset(settings: &AppSettings) -> Preset {
    settings
        .presets
        .iter()
        .find(|p| p.id == settings.active_preset_id)
        .cloned()
        .or_else(|| settings.presets.first().cloned())
        .unwrap_or_else(default_voice_preset)
}

pub fn to_view(settings: &AppSettings) -> SettingsView {
    SettingsView {
        theme: settings.theme.clone(),
        input_device_id: settings.input_device_id.clone(),
        output_device_id: settings.output_device_id.clone(),
        output_dir: settings.output_dir.clone(),
        default_output_dir: default_output_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        active_preset_id: settings.active_preset_id.clone(),
        presets: settings.presets.clone(),
        shortcut: settings.shortcut.clone(),
        indicator: settings.indicator.clone(),
        launch_at_login: settings.launch_at_login,
        wake_on_voice: settings.wake_on_voice,
        onboarded: settings.onboarded,
        onboarding_version: settings.onboarding_version,
    }
}

pub fn wants_pill(app: &AppHandle) -> bool {
    current(app).indicator != "tray"
}

fn slug_id(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        format!("preset-{}", chrono::Local::now().timestamp_millis())
    } else {
        s
    }
}

pub fn preset_slug(preset: &Preset) -> String {
    slug_id(&preset.name)
}

fn ensure_builtin_channels(preset: &mut Preset) {
    if !preset.channels.iter().any(|c| c.kind == "mic") {
        preset.channels.insert(
            0,
            Channel {
                id: "mic".into(),
                kind: "mic".into(),
                enabled: true,
                name: "My voice".into(),
                exe: String::new(),
            },
        );
    }
    if !preset.channels.iter().any(|c| c.kind == "system") {
        let idx = if preset.channels.first().map(|c| c.kind.as_str()) == Some("mic") {
            1
        } else {
            0
        };
        preset.channels.insert(
            idx,
            Channel {
                id: "system".into(),
                kind: "system".into(),
                enabled: false,
                name: "Computer sound".into(),
                exe: String::new(),
            },
        );
    }
    for ch in &mut preset.channels {
        if ch.kind == "mic" && ch.name.is_empty() {
            ch.name = "My voice".into();
        }
        if ch.kind == "system" && ch.name.is_empty() {
            ch.name = "Computer sound".into();
        }
        if ch.kind == "app" && ch.id.is_empty() {
            ch.id = format!("app:{}", ch.exe.to_ascii_lowercase());
        }
    }
}

fn normalize(mut settings: AppSettings) -> AppSettings {
    let theme = settings.theme.to_lowercase();
    settings.theme = match theme.as_str() {
        "light" | "dark" | "system" => theme,
        _ => "system".into(),
    };
    if settings.presets.is_empty() {
        settings.presets.push(default_voice_preset());
    }
    for preset in &mut settings.presets {
        if preset.id.trim().is_empty() {
            preset.id = slug_id(&preset.name);
        }
        if preset.name.trim().is_empty() {
            preset.name = "Untitled".into();
        }
        ensure_builtin_channels(preset);
    }
    if !settings
        .presets
        .iter()
        .any(|p| p.id == settings.active_preset_id)
    {
        settings.active_preset_id = settings.presets[0].id.clone();
    }
    settings.shortcut = settings.shortcut.trim().to_lowercase().replace(' ', "");
    if settings.shortcut.parse::<Shortcut>().is_err() {
        settings.shortcut = default_shortcut();
    }
    settings.indicator = match settings.indicator.to_lowercase().as_str() {
        "tray" => "tray".into(),
        _ => "pill".into(),
    };
    settings
}

pub fn apply_theme(app: &AppHandle, theme: &str) {
    let native = match theme {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    };
    for label in ["settings", "onboarding"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_theme(native);
        }
    }
}

pub fn mark_onboarded(app: &AppHandle) {
    let mut settings = current(app);
    if settings.onboarded && settings.onboarding_version >= ONBOARDING_VERSION {
        return;
    }
    settings.onboarded = true;
    settings.onboarding_version = ONBOARDING_VERSION;
    if save(&settings).is_err() {
        return;
    }
    if let Ok(mut guard) = app.state::<SettingsState>().0.lock() {
        *guard = settings.clone();
    }
    let _ = app.emit("cue://settings-changed", to_view(&settings));
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    to_view(&current(&app))
}

fn apply_shortcut(app: &AppHandle, previous: &str, next: &str) -> Result<(), String> {
    if previous == next {
        return Ok(());
    }
    let parsed: Shortcut = next
        .parse()
        .map_err(|_| "That shortcut isn't valid.".to_string())?;
    let gs = app.global_shortcut();
    if let Ok(old) = previous.parse::<Shortcut>() {
        let _ = gs.unregister(old);
    }
    if let Err(e) = gs.register(parsed) {
        if let Ok(old) = previous.parse::<Shortcut>() {
            let _ = gs.register(old);
        }
        return Err(format!("Couldn't use that shortcut. It may already be taken. ({e})"));
    }
    Ok(())
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    let auto = app.autolaunch();
    if enabled {
        let _ = auto.enable();
    } else {
        let _ = auto.disable();
    }
}

pub fn apply_runtime(app: &AppHandle, settings: &AppSettings) {
    apply_theme(app, &settings.theme);
    apply_autostart(app, settings.launch_at_login);
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: AppSettings) -> Result<SettingsView, String> {
    let previous = current(&app);
    let mut settings = normalize(settings);
    if previous.onboarded {
        settings.onboarded = true;
    }
    if previous.onboarding_version > settings.onboarding_version {
        settings.onboarding_version = previous.onboarding_version;
    }
    apply_shortcut(&app, &previous.shortcut, &settings.shortcut)?;
    apply_autostart(&app, settings.launch_at_login);
    save(&settings)?;
    if let Ok(mut guard) = app.state::<SettingsState>().0.lock() {
        *guard = settings.clone();
    }
    apply_theme(&app, &settings.theme);
    crate::vosk_recognizer::update(&app);
    let view = to_view(&settings);
    let _ = app.emit("cue://settings-changed", view.clone());
    Ok(view)
}

#[tauri::command]
pub fn pick_output_dir(app: AppHandle) -> Result<Option<String>, String> {
    let current = resolved_output_dir(&current(&app)).ok();
    let mut dialog = rfd::FileDialog::new().set_title("Recordings folder");
    if let Some(dir) = current {
        dialog = dialog.set_directory(dir);
    }
    Ok(dialog.pick_folder().map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub fn open_output_dir(app: AppHandle) -> Result<(), String> {
    let path = resolved_output_dir(&current(&app))?;
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn reveal_saved(app: AppHandle, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if path.trim().is_empty() {
            return open_output_dir(app);
        }
        app.opener()
            .reveal_item_in_dir(&path)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_saved(app: AppHandle, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if path.trim().is_empty() {
            return Err("Nothing to play.".into());
        }
        app.opener()
            .open_path(path, None::<&str>)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
