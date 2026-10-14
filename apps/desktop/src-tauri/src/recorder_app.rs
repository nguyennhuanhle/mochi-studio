//! Lớp app của Người quay: lệnh Tauri, chuyển tiếp sự kiện, điều phối cửa sổ, phím tắt.
//! Logic quay nằm trong crate `mochi-recorder`; file này chỉ nối nó với giao diện.

use crate::settings::{SettingsState, default_output_root};
use mochi_recorder::{
    CaptureTarget, DeviceKind, FinishedPaths, RecorderError, RecorderEvent, RecorderService,
    RecorderState, RecordingConfig,
    recovery::{self, RecoverableRecording},
    targets::{self, DeviceList, DisplayInfo, WindowInfo},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

pub const MAIN: &str = "main";
pub const RECORDER_BAR: &str = "recorder-bar";
pub const COUNTDOWN: &str = "countdown";
pub const REGION_PICKER: &str = "region-picker";

/// Sự kiện của RecorderService gửi sang UI (mọi cửa sổ).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct RecorderEventMsg(pub RecorderEvent);

/// Kết quả khung chọn vùng: None = người dùng hủy.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct RegionSelected(pub Option<CaptureTarget>);

pub struct RecorderAppState {
    pub service: RecorderService,
    /// Màn hình đang quay (để đặt thanh điều khiển / đếm ngược đúng chỗ).
    active_display: std::sync::Mutex<Option<DisplayInfo>>,
}

impl RecorderAppState {
    pub fn new() -> Self {
        Self {
            service: RecorderService::new(),
            active_display: std::sync::Mutex::new(None),
        }
    }
}

/// Gọi một lần trong `setup`: chuyển tiếp sự kiện + điều phối cửa sổ + phím tắt.
pub fn setup(app: &AppHandle) {
    let state = app.state::<RecorderAppState>();
    let mut rx = state.service.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok(event) = rx.recv().await {
            on_recorder_event(&handle, &event);
            RecorderEventMsg(event).emit(&handle).ok();
        }
    });
    let conflicts = apply_hotkeys(app);
    if !conflicts.is_empty() {
        tracing::warn!(?conflicts, "phím tắt bị trùng");
    }
}

fn on_recorder_event(app: &AppHandle, event: &RecorderEvent) {
    match event {
        RecorderEvent::StateChanged { state } => {
            crate::jobs_app::on_recorder_state(app, *state);
            crate::captions_app::on_recorder_state(*state);
            match state {
            RecorderState::Countdown => {
                hide_main_for_recording(app);
                show_recorder_bar(app);
                show_countdown(app);
            }
            RecorderState::Recording | RecorderState::Paused => {
                close_window(app, COUNTDOWN);
            }
            RecorderState::Idle => {
                close_window(app, COUNTDOWN);
                close_window(app, RECORDER_BAR);
                restore_main(app);
            }
            RecorderState::Preparing | RecorderState::Finalizing => {}
            }
        }
        RecorderEvent::Finished { recording } => {
            crate::editor_app::precreate_project(app, recording.project_dir.clone());
        }
        _ => {}
    }
}

// ---------------- lệnh cho UI ----------------

#[tauri::command]
#[specta::specta]
pub fn recorder_list_displays() -> Vec<DisplayInfo> {
    targets::list_displays()
}

#[tauri::command]
#[specta::specta]
pub fn recorder_list_windows() -> Vec<WindowInfo> {
    targets::list_windows()
}

#[tauri::command]
#[specta::specta]
pub fn recorder_list_devices() -> DeviceList {
    targets::list_devices()
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_thumbnail(target: CaptureTarget) -> Result<String, RecorderError> {
    targets::thumbnail(&target, 480).await
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_state(state: State<'_, RecorderAppState>) -> Result<RecorderState, ()> {
    Ok(state.service.state().await)
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_start(
    state: State<'_, RecorderAppState>,
    settings: State<'_, SettingsState>,
    config: RecordingConfig,
) -> Result<FinishedPaths, RecorderError> {
    start_recording(&state, &settings, config).await
}

async fn start_recording(
    state: &RecorderAppState,
    settings: &SettingsState,
    config: RecordingConfig,
) -> Result<FinishedPaths, RecorderError> {
    settings
        .update(|s| s.last_recording = Some(config.clone()))
        .ok();
    *state.active_display.lock().unwrap() = display_of(&config.target);
    mochi_recorder::meter::system_audio_meter_stop();
    let name = chrono::Local::now().format("Mochi %Y-%m-%d %H.%M.%S").to_string();
    state.service.start(config, name).await
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_pause(state: State<'_, RecorderAppState>) -> Result<(), RecorderError> {
    state.service.pause().await
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_resume(state: State<'_, RecorderAppState>) -> Result<(), RecorderError> {
    state.service.resume().await
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_stop(
    state: State<'_, RecorderAppState>,
) -> Result<mochi_recorder::FinishedRecording, RecorderError> {
    state.service.stop().await
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_cancel(state: State<'_, RecorderAppState>) -> Result<(), RecorderError> {
    state.service.cancel().await
}

#[tauri::command]
#[specta::specta]
pub async fn recorder_set_mic_muted(
    state: State<'_, RecorderAppState>,
    muted: bool,
) -> Result<(), RecorderError> {
    state.service.set_mic_muted(muted).await
}

/// Bỏ bản quay vừa xong (ví dụ bản quá ngắn).
#[tauri::command]
#[specta::specta]
pub async fn recorder_discard(
    app: AppHandle,
    state: State<'_, RecorderAppState>,
    settings: State<'_, SettingsState>,
    project_dir: PathBuf,
) -> Result<(), RecorderError> {
    let root = output_root(&app, &settings);
    let _ = state;
    // Chờ bước tạo sẵn project (S04) xong: nó đang giữ file .lock trong thư mục sắp xóa.
    let handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let editor = handle.state::<crate::editor_app::EditorAppState>();
        let _wait = editor.precreate.lock().unwrap();
        crate::editor_app::release_player_media(&handle);
        mochi_recorder::finalize::remove_project_dir(&root, &project_dir).map_err(RecorderError::internal)
    })
    .await
    .map_err(RecorderError::internal)?
}

#[tauri::command]
#[specta::specta]
pub fn recovery_list(app: AppHandle, settings: State<'_, SettingsState>) -> Vec<RecoverableRecording> {
    recovery::list(&output_root(&app, &settings))
}

#[tauri::command]
#[specta::specta]
pub async fn recovery_restore(recording_dir: PathBuf) -> Result<(), RecorderError> {
    tokio::task::spawn_blocking(move || recovery::restore(&recording_dir))
        .await
        .map_err(RecorderError::internal)?
}

#[tauri::command]
#[specta::specta]
pub fn recovery_discard(
    app: AppHandle,
    settings: State<'_, SettingsState>,
    recording_dir: PathBuf,
) -> Result<(), RecorderError> {
    recovery::discard(&output_root(&app, &settings), &recording_dir)
}

/// R10 (v1): máy có thu riêng tiếng một app được không (Windows 10 2004+).
#[tauri::command]
#[specta::specta]
pub fn app_audio_supported() -> bool {
    mochi_recorder::app_audio::supported()
}

/// R02: mức đỉnh của âm thanh hệ thống (bật thu loopback nếu chưa bật). UI gọi ~10 lần/giây.
#[tauri::command]
#[specta::specta]
pub fn system_audio_level() -> Result<mochi_recorder::meter::SystemAudioLevel, String> {
    mochi_recorder::meter::system_audio_level()
}

#[tauri::command]
#[specta::specta]
pub fn system_audio_meter_stop() {
    mochi_recorder::meter::system_audio_meter_stop();
}

/// Mở trang quyền riêng tư của Windows (lỗi "chưa cấp quyền").
#[tauri::command]
#[specta::specta]
pub fn open_privacy_settings(app: AppHandle, kind: DeviceKind) -> Result<(), String> {
    let uri = match kind {
        DeviceKind::Camera => "ms-settings:privacy-webcam",
        DeviceKind::Microphone => "ms-settings:privacy-microphone",
        _ => "ms-settings:privacy",
    };
    app.opener()
        .open_url(uri, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn reveal_path(app: AppHandle, path: PathBuf) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

/// Phím tắt không đăng ký được (app khác đang dùng) + phím gợi ý còn trống.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflict {
    pub key: String,
    pub suggestion: Option<String>,
}

/// Đăng ký lại phím tắt theo settings; trả về các phím bị trùng với app khác.
#[tauri::command]
#[specta::specta]
pub fn hotkeys_apply(app: AppHandle) -> Vec<HotkeyConflict> {
    apply_hotkeys(&app)
}

/// Tạm nhả mọi phím tắt toàn cục (trong lúc người dùng bấm phím mới ở hộp thoại đổi phím,
/// để F9… không kích hoạt quay). `hotkeys_apply` / `hotkeys_set` đăng ký lại.
#[tauri::command]
#[specta::specta]
pub fn hotkeys_suspend(app: AppHandle) {
    app.global_shortcut().unregister_all().ok();
}

/// R04: đổi phím tắt. Phím sai cú pháp hoặc hai thao tác cùng một phím → lỗi (không lưu).
/// Lưu xong đăng ký lại; trả về các phím bị app khác giữ (kèm gợi ý).
#[tauri::command]
#[specta::specta]
pub fn hotkeys_set(
    app: AppHandle,
    settings: State<'_, SettingsState>,
    hotkeys: crate::settings::Hotkeys,
) -> Result<Vec<HotkeyConflict>, String> {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::Shortcut;
    let keys = [&hotkeys.record_pause, &hotkeys.stop, &hotkeys.cancel];
    let mut parsed = Vec::new();
    for k in keys {
        if k.trim().is_empty() {
            continue;
        }
        let sc = Shortcut::from_str(k).map_err(|_| format!("invalid:{k}"))?;
        if parsed.contains(&sc) {
            return Err(format!("duplicate:{k}"));
        }
        parsed.push(sc);
    }
    settings
        .update(|s| s.hotkeys = hotkeys)
        .map_err(|e| e.to_string())?;
    Ok(apply_hotkeys(&app))
}

// ---------------- khung chọn vùng ----------------

#[tauri::command]
#[specta::specta]
pub fn region_picker_open(app: AppHandle, display_id: String) -> Result<(), String> {
    let display = targets::list_displays()
        .into_iter()
        .find(|d| d.id == display_id)
        .ok_or("không thấy màn hình")?;
    close_window(&app, REGION_PICKER);
    let route = format!("/region-picker?display={display_id}");
    let window = overlay_builder(&app, REGION_PICKER, &route)
        .focused(true)
        .build()
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(
            display.physical_x as i32,
            display.physical_y as i32,
        ))
        .ok();
    window
        .set_size(PhysicalSize::new(
            display.physical_width as u32,
            display.physical_height as u32,
        ))
        .ok();
    window.show().ok();
    window.set_focus().ok();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn region_picker_finish(app: AppHandle, area: Option<CaptureTarget>) {
    close_window(&app, REGION_PICKER);
    RegionSelected(area).emit(&app).ok();
    if let Some(main) = app.get_webview_window(MAIN) {
        main.set_focus().ok();
    }
}

// ---------------- điều phối cửa sổ ----------------

pub fn output_root(app: &AppHandle, settings: &SettingsState) -> PathBuf {
    settings
        .get()
        .output_root
        .unwrap_or_else(|| default_output_root(app))
}

fn display_of(target: &CaptureTarget) -> Option<DisplayInfo> {
    let displays = targets::list_displays();
    let id = match target {
        CaptureTarget::Display { id } => Some(id.clone()),
        CaptureTarget::Area { display_id, .. } => Some(display_id.clone()),
        CaptureTarget::Window { .. } => None,
    };
    match id {
        Some(id) => displays.into_iter().find(|d| d.id == id),
        None => displays.into_iter().next(),
    }
}

/// Cửa sổ trong suốt, không viền, luôn trên cùng, không hiện trên taskbar.
fn overlay_builder<'a>(app: &'a AppHandle, label: &'a str, route: &str) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!(
            "if (!location.hash || location.hash === '#/') location.hash = '#{route}';"
        ))
        .title("Mochi Studio")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
}

fn show_recorder_bar(app: &AppHandle) {
    if app.get_webview_window(RECORDER_BAR).is_some() {
        return;
    }
    let Ok(window) = overlay_builder(app, RECORDER_BAR, "/recorder-bar")
        .inner_size(460.0, 64.0)
        .focused(false)
        .build()
    else {
        return;
    };
    // R07: thanh điều khiển không lọt vào bản quay.
    window.set_content_protected(true).ok();
    place_on_active_display(app, &window, Placement::BottomCenter);
    window.show().ok();
}

fn show_countdown(app: &AppHandle) {
    if app.get_webview_window(COUNTDOWN).is_some() {
        return;
    }
    let Ok(window) = overlay_builder(app, COUNTDOWN, "/countdown")
        .inner_size(240.0, 240.0)
        .focused(false)
        .build()
    else {
        return;
    };
    window.set_content_protected(true).ok();
    window.set_ignore_cursor_events(true).ok();
    place_on_active_display(app, &window, Placement::Center);
    window.show().ok();
}

enum Placement {
    Center,
    BottomCenter,
}

fn place_on_active_display(app: &AppHandle, window: &WebviewWindow, placement: Placement) {
    let display = app
        .state::<RecorderAppState>()
        .active_display
        .lock()
        .unwrap()
        .clone();
    let Some(d) = display else { return };
    let Ok(size) = window.outer_size() else { return };
    let x = d.physical_x + (d.physical_width - size.width as f64) / 2.0;
    let y = match placement {
        Placement::Center => d.physical_y + (d.physical_height - size.height as f64) / 2.0,
        Placement::BottomCenter => {
            d.physical_y + d.physical_height - size.height as f64 - 0.08 * d.physical_height
        }
    };
    window
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .ok();
}

fn hide_main_for_recording(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN) {
        // Phòng khi người dùng mở lại cửa sổ chính trong lúc quay: vẫn không lọt vào bản quay.
        main.set_content_protected(true).ok();
        main.minimize().ok();
    }
}

fn restore_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN) {
        main.set_content_protected(false).ok();
        main.unminimize().ok();
        main.show().ok();
        main.set_focus().ok();
    }
}

fn close_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        w.destroy().ok();
    }
}

// ---------------- phím tắt toàn cục (R04) ----------------

#[derive(Clone, Copy)]
enum HotkeyAction {
    RecordPause,
    Stop,
    Cancel,
}

fn apply_hotkeys(app: &AppHandle) -> Vec<HotkeyConflict> {
    let gs = app.global_shortcut();
    gs.unregister_all().ok();
    let hotkeys = app.state::<SettingsState>().get().hotkeys;
    let mut conflicts = Vec::new();
    let ours = [hotkeys.record_pause.clone(), hotkeys.stop.clone(), hotkeys.cancel.clone()];
    for (accel, action) in [
        (hotkeys.record_pause, HotkeyAction::RecordPause),
        (hotkeys.stop, HotkeyAction::Stop),
        (hotkeys.cancel, HotkeyAction::Cancel),
    ] {
        if accel.trim().is_empty() {
            continue;
        }
        let result = gs.on_shortcut(accel.as_str(), move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { run_hotkey(app, action).await });
            }
        });
        if result.is_err() {
            let suggestion = suggest_hotkey(app, &accel, &ours);
            conflicts.push(HotkeyConflict { key: accel, suggestion });
        }
    }
    conflicts
}

/// Phím thay thế còn trống cho `accel`: cùng phím chính với tổ hợp bổ trợ khác, rồi tới các phím F
/// khác. "Còn trống" = đăng ký thử được (rồi nhả ngay).
fn suggest_hotkey(app: &AppHandle, accel: &str, ours: &[String]) -> Option<String> {
    let gs = app.global_shortcut();
    let key = accel.rsplit('+').next().unwrap_or(accel).trim().to_string();
    let mut candidates: Vec<String> = ["Ctrl+Shift", "Ctrl+Alt", "Alt+Shift"]
        .iter()
        .map(|m| format!("{m}+{key}"))
        .collect();
    candidates.extend((6..=12).map(|n| format!("Ctrl+Shift+F{n}")));
    candidates.into_iter().find(|c| {
        if ours.iter().any(|o| o.eq_ignore_ascii_case(c)) {
            return false;
        }
        match gs.register(c.as_str()) {
            Ok(()) => {
                gs.unregister(c.as_str()).ok();
                true
            }
            Err(_) => false,
        }
    })
}

async fn run_hotkey(app: AppHandle, action: HotkeyAction) {
    let state = app.state::<RecorderAppState>();
    let settings = app.state::<SettingsState>();
    let service = &state.service;
    let result = match (action, service.state().await) {
        (HotkeyAction::RecordPause, RecorderState::Idle) => {
            match settings.get().last_recording {
                Some(config) => start_recording(&state, &settings, config).await.map(|_| ()),
                None => {
                    // Chưa có cấu hình nào: mở màn thiết lập quay.
                    restore_main(&app);
                    if let Some(main) = app.get_webview_window(MAIN) {
                        main.eval("location.hash = '#/record'").ok();
                    }
                    Ok(())
                }
            }
        }
        (HotkeyAction::RecordPause, RecorderState::Recording) => service.pause().await,
        (HotkeyAction::RecordPause, RecorderState::Paused) => service.resume().await,
        // Dừng khi còn đang đếm ngược: chưa ghi gì → hủy, không báo lỗi "không có phiên quay".
        (HotkeyAction::Stop, RecorderState::Countdown) => service.cancel().await,
        (HotkeyAction::Stop, s) if s.is_active() => service.stop().await.map(|_| ()),
        (HotkeyAction::Cancel, s) if s.is_active() => service.cancel().await,
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!("hotkey: {e}");
        // Lỗi khi bắt đầu bằng phím tắt: hiện lại cửa sổ chính để người dùng thấy lỗi.
        restore_main(&app);
        RecorderEventMsg(RecorderEvent::Failed { error: e }).emit(&app).ok();
    }
}
