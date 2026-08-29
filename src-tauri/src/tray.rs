use tauri::{image::Image, AppHandle};

pub const TRAY_ID: &str = "cue";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Recording,
    Error,
}

fn icon(state: TrayState) -> Image<'static> {
    match state {
        TrayState::Idle => tauri::include_image!("icons/tray/tray-idle@2x.png"),
        TrayState::Recording => tauri::include_image!("icons/tray/tray-recording@2x.png"),
        TrayState::Error => tauri::include_image!("icons/tray/tray-error@2x.png"),
    }
}

fn tooltip(state: TrayState) -> &'static str {
    match state {
        TrayState::Idle => "Cue",
        TrayState::Recording => "Cue · Recording",
        TrayState::Error => "Cue · Couldn't record",
    }
}

pub fn idle_icon() -> Image<'static> {
    icon(TrayState::Idle)
}

pub fn idle_tooltip() -> &'static str {
    tooltip(TrayState::Idle)
}

pub fn set_state(app: &AppHandle, state: TrayState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_icon(Some(icon(state)));
    let _ = tray.set_tooltip(Some(tooltip(state)));
}
