//! Lớp app của Người biên tập: lệnh Tauri cho phiên biên tập, trình phát preview, autosave.
//! Logic nằm ở `mochi-editor` (project + timeline) và `mochi-preview` (phát).

use crate::settings::SettingsState;
use base64::Engine;
use mochi_editor::{EditorError, EditorSession, ImportReport, ProjectView, RelinkReport};
use mochi_preview::{PhysRect, PlayerCommand, PlayerEvent, PreviewPlayer};
use mochi_project::{BackupInfo, ProjectError, Ticks};
use mochi_render::GpuContext;
use mochi_timeline::EditOp;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

#[derive(Default)]
pub struct EditorAppState {
    /// Giữ trong lúc tạo sẵn project ngay sau khi dừng quay — `project_open` chờ xong mới mở.
    pub precreate: Mutex<()>,
    session: Mutex<Option<EditorSession>>,
    player: Mutex<Option<PreviewPlayer>>,
    preview_hwnd: Mutex<Option<isize>>,
    /// UI đang có hộp thoại đè lên → preview native phải ẩn, kể cả khi HWND được tạo sau đó.
    preview_hidden: std::sync::atomic::AtomicBool,
    gpu: Mutex<Option<GpuContext>>,
    /// S03: proxy đã sẵn sàng (media id → file) — chỉ preview dùng, xuất luôn đọc file gốc.
    proxies: Mutex<HashMap<String, PathBuf>>,
}

/// Vị trí phát (gửi ~30 lần/giây khi đang phát).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTime {
    pub t: Ticks,
    pub playing: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStats {
    pub fps: f32,
    pub render_ms: f32,
}

/// S01: kết quả tự lưu.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct AutosaveStatus {
    pub ok: bool,
    pub at: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RectArg {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl From<RectArg> for PhysRect {
    fn from(r: RectArg) -> Self {
        PhysRect { x: r.x, y: r.y, width: r.width, height: r.height }
    }
}

type Res<T> = Result<T, EditorError>;

fn no_project() -> EditorError {
    EditorError::Project { error: ProjectError::NotFound }
}

impl EditorAppState {
    fn with<T>(&self, f: impl FnOnce(&mut EditorSession) -> Res<T>) -> Res<T> {
        let mut guard = self.session.lock().unwrap();
        f(guard.as_mut().ok_or_else(no_project)?)
    }

    /// Đọc phiên đang mở (nếu có).
    pub fn with_session<T>(&self, f: impl FnOnce(&EditorSession) -> T) -> Option<T> {
        self.session.lock().unwrap().as_ref().map(f)
    }

    /// Kiểm GPU (dùng chung GPU của preview, tạo nếu chưa có).
    pub fn gpu_check(&self) -> mochi_render::GpuCheck {
        match self.gpu() {
            Ok(g) => g.check(),
            Err(e) => mochi_render::GpuCheck { ok: false, adapter: format!("{e:#}"), driver: String::new(), software: false, old_driver: false },
        }
    }

    fn gpu(&self) -> anyhow::Result<GpuContext> {
        let mut g = self.gpu.lock().unwrap();
        if g.is_none() {
            *g = Some(GpuContext::new()?);
        }
        Ok(g.clone().unwrap())
    }

    /// Gửi project mới nhất (kèm đường dẫn thật của media) cho trình phát.
    fn sync_player(&self) {
        let snapshot = {
            let guard = self.session.lock().unwrap();
            guard.as_ref().map(|s| {
                let p = s.project().clone();
                let paths: HashMap<String, PathBuf> = p
                    .media
                    .iter()
                    .filter_map(|m| s.media_path(&m.id).map(|path| (m.id.clone(), path)))
                    .collect();
                (Arc::new(p), paths)
            })
        };
        let proxies = self.proxies.lock().unwrap().clone();
        if let (Some((project, paths)), Some(player)) = (snapshot, self.player.lock().unwrap().as_ref()) {
            player.send(PlayerCommand::SetProject { project, paths, proxies });
        }
    }

    /// Cập nhật danh sách proxy rồi gửi lại cho trình phát (preview chuyển sang / bỏ proxy).
    pub fn set_proxies(&self, proxies: HashMap<String, PathBuf>) {
        *self.proxies.lock().unwrap() = proxies;
        self.sync_player();
    }

    /// Tạo thread trình phát nếu chưa có (chưa cần HWND: không có HWND thì chỉ render, không hiện).
    fn ensure_player(&self, app: &AppHandle) -> anyhow::Result<()> {
        let mut player = self.player.lock().unwrap();
        if player.is_none() {
            let gpu = self.gpu()?;
            let handle = app.clone();
            *player = Some(PreviewPlayer::spawn(
                gpu,
                tauri::async_runtime::handle().inner().clone(),
                move |e| match e {
                    PlayerEvent::Time { t, playing } => {
                        PlayerTime { t, playing }.emit(&handle).ok();
                    }
                    PlayerEvent::Stats { fps, render_ms, .. } => {
                        PlayerStats { fps, render_ms }.emit(&handle).ok();
                    }
                },
            ));
        }
        Ok(())
    }

    fn player_send(&self, cmd: PlayerCommand) {
        if let Some(p) = self.player.lock().unwrap().as_ref() {
            p.send(cmd);
        }
    }
}

/// Đóng cửa sổ chính khi project đang mở chưa lưu được.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct CloseSaveFailed {
    pub error: EditorError,
}

/// Lưu rồi đóng project đang mở. Lưu lỗi → **giữ phiên mở** và trả lỗi (S01: không được mất thay
/// đổi âm thầm); chỉ bỏ thay đổi khi người dùng chọn rõ (`discard`).
fn close_session(state: &EditorAppState, discard: bool) -> Res<()> {
    let mut guard = state.session.lock().unwrap();
    if let Some(s) = guard.as_mut()
        && s.is_dirty()
        && !discard
    {
        s.save(true)?;
    }
    // Lấy phiên ra rồi drop: `.lock` được ghi `state=closed`. Nếu để phiên trong state, tiến trình
    // thoát không chạy destructor → lần mở sau tưởng là crash (báo "đã khôi phục" sai).
    let session = guard.take();
    drop(guard);
    if discard && session.as_ref().is_some_and(|s| s.is_dirty()) {
        tracing::warn!("đóng project, bỏ thay đổi chưa lưu (người dùng chọn)");
    }
    drop(session);
    Ok(())
}

/// Khi đóng cửa sổ chính: lưu project đang mở. Lỗi → trả về để chặn việc đóng cửa sổ.
pub fn save_on_exit(app: &AppHandle) -> Res<()> {
    close_session(&app.state::<EditorAppState>(), false).inspect_err(|e| {
        tracing::error!("lưu khi thoát thất bại: {e}");
    })
}

/// Hộp thoại "không lưu được" lúc thoát: thử lưu lại (`discard=false`) hoặc bỏ thay đổi rồi thoát.
#[tauri::command]
#[specta::specta]
pub fn app_exit(app: AppHandle, state: State<'_, EditorAppState>, discard: bool) -> Res<()> {
    close_session(&state, discard)?;
    app.exit(0);
    Ok(())
}

/// Gọi một lần trong `setup`: vòng autosave nền (S01).
pub fn setup(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            // Nhịp 2 s: thay đổi lớn được lưu sau ~2 s (S01); lỗi thì phiên tự chờ 10 s mới thử lại.
            tokio::time::sleep(Duration::from_secs(2)).await;
            let state = handle.state::<EditorAppState>();
            let result = {
                let mut guard = state.session.lock().unwrap();
                guard.as_mut().and_then(|s| s.autosave_if_due())
            };
            if let Some(r) = result {
                AutosaveStatus {
                    ok: r.is_ok(),
                    at: chrono::Local::now().format("%H:%M:%S").to_string(),
                    error: r.err().map(|e| e.to_string()),
                }
                .emit(&handle)
                .ok();
            }
        }
    });
}

fn open_session(app: &AppHandle, state: &EditorAppState, session: EditorSession) -> ProjectView {
    let view = session.view();
    if let Some(old) = state.session.lock().unwrap().replace(session) {
        let mut old = old;
        if old.is_dirty() {
            let _ = old.save(true);
        }
    }
    remember_recent(app, &view.dir);
    state.sync_player();
    crate::jobs_app::enqueue_for_open_project(app);
    crate::jobs_app::refresh_preview(app);
    view
}

fn remember_recent(app: &AppHandle, dir: &std::path::Path) {
    app.state::<SettingsState>()
        .update(|s| {
            s.recent_projects.retain(|p| p != dir);
            s.recent_projects.insert(0, dir.to_path_buf());
            s.recent_projects.truncate(12);
        })
        .ok();
}

// ---------------- project ----------------

#[tauri::command]
#[specta::specta]
pub fn project_open(app: AppHandle, state: State<'_, EditorAppState>, dir: PathBuf) -> Res<ProjectView> {
    let _wait = state.precreate.lock().unwrap();
    // Mở lại đúng project đang mở trong app → dùng phiên hiện tại (K-E3 chỉ chặn process khác).
    if let Some(s) = state.session.lock().unwrap().as_ref()
        && s.dir() == dir
    {
        return Ok(s.view());
    }
    let session = EditorSession::open(&dir)?;
    Ok(open_session(&app, &state, session))
}

#[tauri::command]
#[specta::specta]
pub fn project_open_backup(
    app: AppHandle,
    state: State<'_, EditorAppState>,
    dir: PathBuf,
    backup: PathBuf,
) -> Res<ProjectView> {
    // Đang mở đúng project này (vd. vừa khôi phục sau crash, người dùng chọn bản khác): nhả phiên
    // hiện tại trước để giành lại khóa. project.json hiện tại được `open_backup` giữ thành backup.
    {
        let mut guard = state.session.lock().unwrap();
        if guard.as_ref().is_some_and(|s| same_dir(s.dir(), &dir)) {
            drop(guard.take());
        }
    }
    let session = EditorSession::open_backup(&dir, &backup)?;
    Ok(open_session(&app, &state, session))
}

fn same_dir(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// S02: project gần đây mà lần trước không đóng sạch (app crash / mất điện) — trang chủ đề nghị mở
/// lại để khôi phục bản tự lưu.
#[tauri::command]
#[specta::specta]
pub fn projects_unclean(app: AppHandle, state: State<'_, EditorAppState>) -> Vec<PathBuf> {
    let open = state.session.lock().unwrap().as_ref().map(|s| s.dir().to_path_buf());
    app.state::<SettingsState>()
        .get()
        .recent_projects
        .into_iter()
        .filter(|d| !open.as_deref().is_some_and(|o| same_dir(o, d)))
        .filter(|d| mochi_project::package::is_unclean(d))
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn project_create(app: AppHandle, state: State<'_, EditorAppState>, name: String) -> Res<ProjectView> {
    let root = app
        .state::<SettingsState>()
        .get()
        .output_root
        .unwrap_or_else(|| crate::settings::default_output_root(&app));
    let base = mochi_recorder::finalize::sanitize_file_name(&name);
    let mut dir = root.join(format!("{base}.mochi"));
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{base} ({n}).mochi"));
        n += 1;
    }
    let session = EditorSession::create(&dir, &name)?;
    Ok(open_session(&app, &state, session))
}

#[tauri::command]
#[specta::specta]
pub fn project_view(state: State<'_, EditorAppState>) -> Option<ProjectView> {
    state.session.lock().unwrap().as_ref().map(|s| s.view())
}

#[tauri::command]
#[specta::specta]
pub fn project_save(state: State<'_, EditorAppState>) -> Res<ProjectView> {
    state.with(|s| {
        s.save(true)?;
        Ok(s.view())
    })
}

#[tauri::command]
#[specta::specta]
pub fn project_close(state: State<'_, EditorAppState>, discard: bool) -> Res<()> {
    close_session(&state, discard)
}

#[tauri::command]
#[specta::specta]
pub fn project_backups(state: State<'_, EditorAppState>) -> Vec<BackupInfo> {
    state.session.lock().unwrap().as_ref().map(|s| s.backups()).unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub fn recent_projects(app: AppHandle) -> Vec<PathBuf> {
    app.state::<SettingsState>()
        .get()
        .recent_projects
        .into_iter()
        .filter(|p| p.join("project.json").is_file() || p.join("recordings").is_dir())
        .collect()
}

// ---------------- media ----------------

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub report: ImportReport,
    pub view: ProjectView,
}

#[tauri::command]
#[specta::specta]
pub fn media_import(app: AppHandle, state: State<'_, EditorAppState>, paths: Vec<PathBuf>) -> Res<ImportResult> {
    let r = state.with(|s| {
        let report = s.import(&paths);
        Ok(ImportResult { report, view: s.view() })
    })?;
    state.sync_player();
    crate::jobs_app::enqueue_for_open_project(&app);
    Ok(r)
}

#[tauri::command]
#[specta::specta]
pub fn media_import_duplicate(app: AppHandle, state: State<'_, EditorAppState>, path: PathBuf) -> Res<ProjectView> {
    let v = state.with(|s| s.import_duplicate(&path))?;
    state.sync_player();
    crate::jobs_app::enqueue_for_open_project(&app);
    Ok(v)
}

#[tauri::command]
#[specta::specta]
pub fn media_relink(app: AppHandle, state: State<'_, EditorAppState>, media_id: String, path: PathBuf) -> Res<ProjectView> {
    let v = state.with(|s| s.relink(&media_id, &path))?;
    state.sync_player();
    crate::jobs_app::enqueue_for_open_project(&app);
    Ok(v)
}

#[tauri::command]
#[specta::specta]
pub fn media_relink_folder(app: AppHandle, state: State<'_, EditorAppState>, folder: PathBuf) -> Res<RelinkReport> {
    let r = state.with(|s| Ok(s.relink_folder(&folder)))?;
    state.sync_player();
    crate::jobs_app::enqueue_for_open_project(&app);
    Ok(r)
}

#[tauri::command]
#[specta::specta]
pub fn media_remove(state: State<'_, EditorAppState>, media_id: String, confirm: bool) -> Res<ProjectView> {
    let v = state.with(|s| s.remove_media(&media_id, confirm))?;
    state.sync_player();
    Ok(v)
}

/// S03: việc nền (ảnh thu nhỏ, waveform) không chạy được lúc này.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum MediaTaskError {
    /// K-S4: đang quay → hoãn, UI hỏi lại khi quay xong.
    Deferred,
    /// Lỗi thật (file hỏng…): UI đánh dấu trên media và cho thử lại.
    Failed { message: String },
}

fn deferred_while_recording(app: &AppHandle) -> Result<(), MediaTaskError> {
    if app.state::<crate::jobs_app::JobsAppState>().scheduler.is_suspended() {
        Err(MediaTaskError::Deferred)
    } else {
        Ok(())
    }
}

/// Ảnh thu nhỏ của media (data URL JPEG), cache trong `cache/thumbs/`. Đang quay → `Deferred`
/// (trừ khi đã có trong cache); lỗi → `Failed` (không ghi cache, thử lại được).
#[tauri::command]
#[specta::specta]
pub async fn media_thumbnail(app: AppHandle, state: State<'_, EditorAppState>, media_id: String) -> Result<Option<String>, MediaTaskError> {
    let job = state
        .with(|s| {
            let item = s.project().media.iter().find(|m| m.id == media_id).cloned();
            Ok(item.zip(s.media_path(&media_id)).map(|(i, p)| (i, p, s.cache_dir())))
        })
        .ok()
        .flatten();
    let Some((item, path, cache)) = job else { return Ok(None) };
    let file = cache.join("thumbs").join(format!("{}.jpg", item.id));
    if !file.is_file() {
        deferred_while_recording(&app)?;
    }
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        if let Ok(b) = std::fs::read(&file) {
            return Ok(b);
        }
        let at = (item.duration.secs() * 0.1).min(2.0);
        let b = mochi_project::media::thumbnail_jpeg(&item, &path, at, 320).map_err(|e| e.to_string())?;
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&file, &b);
        }
        Ok(b)
    })
    .await
    .map_err(|e| MediaTaskError::Failed { message: e.to_string() })?
    .map_err(|message| MediaTaskError::Failed { message })?;
    crate::jobs_app::cache_grew(&app);
    Ok(Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

/// Waveform của media: đỉnh mỗi 100 ms (0–1), cache trong `cache/waveforms/`.
#[tauri::command]
#[specta::specta]
pub async fn media_waveform(app: AppHandle, state: State<'_, EditorAppState>, media_id: String) -> Result<Vec<f32>, MediaTaskError> {
    let job = state
        .with(|s| {
            let item = s.project().media.iter().find(|m| m.id == media_id).cloned();
            Ok(item.zip(s.media_path(&media_id)).map(|(i, p)| (i, p, s.cache_dir())))
        })
        .ok()
        .flatten();
    let Some((item, path, cache)) = job else { return Ok(vec![]) };
    let file = cache.join("waveforms").join(format!("{}.json", item.id));
    if !file.is_file() {
        deferred_while_recording(&app)?;
    }
    let peaks = tokio::task::spawn_blocking(move || -> Result<Vec<f32>, String> {
        if let Some(v) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            return Ok(v);
        }
        let peaks = mochi_preview::waveform(item.kind, &path);
        // Media có luồng tiếng mà không ra đỉnh nào → giải mã lỗi (không cache, cho thử lại).
        if peaks.is_empty() && item.audio.is_some() && item.kind != mochi_project::MediaKind::Recording {
            return Err("không giải mã được luồng tiếng".into());
        }
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&file, serde_json::to_vec(&peaks).unwrap_or_default());
        }
        Ok(peaks)
    })
    .await
    .map_err(|e| MediaTaskError::Failed { message: e.to_string() })?
    .map_err(|message| MediaTaskError::Failed { message })?;
    crate::jobs_app::cache_grew(&app);
    Ok(peaks)
}

#[tauri::command]
#[specta::specta]
pub async fn pick_media_files(app: AppHandle) -> Vec<PathBuf> {
    let mut exts: Vec<&str> = Vec::new();
    exts.extend(mochi_project::media::VIDEO_EXT);
    exts.extend(mochi_project::media::AUDIO_EXT);
    exts.extend(mochi_project::media::IMAGE_EXT);
    app.dialog()
        .file()
        .add_filter("Media", &exts)
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn pick_folder(app: AppHandle) -> Option<PathBuf> {
    app.dialog().file().blocking_pick_folder().and_then(|f| f.into_path().ok())
}

// ---------------- timeline ----------------

#[tauri::command]
#[specta::specta]
pub fn timeline_apply(state: State<'_, EditorAppState>, op: EditOp) -> Res<ProjectView> {
    let v = state.with(|s| s.apply(&op))?;
    state.sync_player();
    Ok(v)
}

/// E05/S04: sinh lại vùng zoom tự động của một clip bản quay (giữ vùng đã sửa tay).
#[tauri::command]
#[specta::specta]
pub fn timeline_regenerate_zoom(state: State<'_, EditorAppState>, clip_id: String) -> Res<ProjectView> {
    let v = state.with(|s| s.regenerate_zoom(&clip_id))?;
    state.sync_player();
    Ok(v)
}

#[tauri::command]
#[specta::specta]
pub fn timeline_undo(state: State<'_, EditorAppState>) -> Res<ProjectView> {
    let v = state.with(|s| s.undo())?;
    state.sync_player();
    Ok(v)
}

#[tauri::command]
#[specta::specta]
pub fn timeline_redo(state: State<'_, EditorAppState>) -> Res<ProjectView> {
    let v = state.with(|s| s.redo())?;
    state.sync_player();
    Ok(v)
}

// ---------------- preview ----------------

/// Gắn preview native vào vùng `rect` của cửa sổ chính (pixel vật lý). Lệnh đồng bộ để chạy
/// trên main thread — thread sở hữu cửa sổ cha.
#[tauri::command]
#[specta::specta]
pub fn preview_attach(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, EditorAppState>,
    rect: RectArg,
) -> Result<(), String> {
    if state.preview_hwnd.lock().unwrap().is_some() {
        preview_resize(state, rect);
        return Ok(());
    }
    let parent = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let hwnd = mochi_preview::host::win::create_child(parent, rect.into())?;
    if state.preview_hidden.load(std::sync::atomic::Ordering::SeqCst) {
        mochi_preview::host::win::set_visible(hwnd, false);
    }
    state.ensure_player(&app).map_err(|e| format!("{e:#}"))?;
    state.player_send(PlayerCommand::Attach { hwnd, width: rect.width, height: rect.height });
    *state.preview_hwnd.lock().unwrap() = Some(hwnd);
    state.sync_player();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn preview_resize(state: State<'_, EditorAppState>, rect: RectArg) {
    if let Some(hwnd) = *state.preview_hwnd.lock().unwrap() {
        mochi_preview::host::win::move_child(hwnd, rect.into());
        state.player_send(PlayerCommand::Resize { width: rect.width, height: rect.height });
    }
}

/// Ẩn tạm preview native khi UI mở hộp thoại đè lên (HTML không vẽ được trên HWND con).
#[tauri::command]
#[specta::specta]
pub fn preview_set_visible(state: State<'_, EditorAppState>, visible: bool) {
    state.preview_hidden.store(!visible, std::sync::atomic::Ordering::SeqCst);
    if let Some(hwnd) = *state.preview_hwnd.lock().unwrap() {
        mochi_preview::host::win::set_visible(hwnd, visible);
    }
}

#[tauri::command]
#[specta::specta]
pub fn preview_detach(state: State<'_, EditorAppState>) {
    let Some(hwnd) = state.preview_hwnd.lock().unwrap().take() else { return };
    state.player_send(PlayerCommand::Pause);
    let (tx, rx) = std::sync::mpsc::channel();
    state.player_send(PlayerCommand::Detach { done: tx });
    let _ = rx.recv_timeout(Duration::from_secs(2));
    mochi_preview::host::win::destroy_child(hwnd);
}

#[tauri::command]
#[specta::specta]
pub fn player_toggle(state: State<'_, EditorAppState>) {
    state.player_send(PlayerCommand::TogglePlay);
}

#[tauri::command]
#[specta::specta]
pub fn player_pause(state: State<'_, EditorAppState>) {
    state.player_send(PlayerCommand::Pause);
}

#[tauri::command]
#[specta::specta]
pub fn player_seek(state: State<'_, EditorAppState>, t: Ticks) {
    state.player_send(PlayerCommand::Seek(t));
}

#[tauri::command]
#[specta::specta]
pub fn player_step(state: State<'_, EditorAppState>, frames: i32) {
    state.player_send(PlayerCommand::Step(frames));
}

/// S04: ngay khi dừng quay, tạo sẵn project.json (auto-polish + zoom tự sinh) ở chế độ nền.
pub fn precreate_project(app: &AppHandle, project_dir: PathBuf) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let state = handle.state::<EditorAppState>();
        let _guard = state.precreate.lock().unwrap();
        if project_dir.join("project.json").is_file() {
            return;
        }
        match EditorSession::open(&project_dir) {
            Ok(s) => {
                tracing::info!("đã tạo sẵn project ({} clip)", s.project().clips.len());
                warm_player(&handle, &state, &s);
            }
            Err(e) => tracing::warn!("tạo sẵn project lỗi: {e}"),
        }
    });
}

/// Làm ấm preview: trình phát (chưa gắn HWND) render frame đầu của project vừa tạo sẵn, để
/// renderer của Cap mở decoder và biên dịch pipeline trước khi người dùng bấm mở trình biên tập.
/// Khi mở thật, `retain_media` giữ lại nguồn đã mở vì id media trùng nhau.
fn warm_player(app: &AppHandle, state: &EditorAppState, s: &EditorSession) {
    // Preview đang hiện (đang biên tập project khác) → không giành trình phát. Khi preview
    // chưa gắn, lần gắn sau `sync_player` sẽ gửi lại project của phiên đang mở.
    if state.preview_hwnd.lock().unwrap().is_some() {
        return;
    }
    if let Err(e) = state.ensure_player(app) {
        tracing::warn!("làm ấm preview lỗi: {e:#}");
        return;
    }
    let project = s.project().clone();
    let paths: HashMap<String, PathBuf> = project
        .media
        .iter()
        .filter_map(|m| s.media_path(&m.id).map(|path| (m.id.clone(), path)))
        .collect();
    state.player_send(PlayerCommand::SetProject { project: Arc::new(project), paths, proxies: HashMap::new() });
    tracing::info!("đang làm ấm preview cho {}", s.dir().display());
}

/// Trước khi xóa một bản quay: trình phát thả mọi file media (Windows không xóa được file đang mở).
pub fn release_player_media(app: &AppHandle) {
    let state = app.state::<EditorAppState>();
    if state.preview_hwnd.lock().unwrap().is_some() {
        return; // preview đang hiện project đang biên tập, không phải bản quay sắp xóa
    }
    let (tx, rx) = std::sync::mpsc::channel();
    state.player_send(PlayerCommand::Unload { done: tx });
    // Trình phát có thể đang giữa frame làm ấm đầu tiên (~7 s) → chờ đủ lâu.
    let _ = rx.recv_timeout(Duration::from_secs(15));
}
