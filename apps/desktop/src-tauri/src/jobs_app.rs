//! Hệ thống (Vòng 5): tác vụ nền (proxy S03), hoãn khi đang quay (K-S4), cache (K-S3, giới hạn 20 GB).

use crate::editor_app::EditorAppState;
use mochi_jobs::{CacheUsage, JobScheduler, JobStatus, ProxyJob, cache};
use mochi_recorder::RecorderState;
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

/// Trạng thái tác vụ nền của một media.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct JobsUpdate {
    pub media_id: String,
    pub status: JobStatus,
}

/// Cache vượt giới hạn → đã xóa proxy cũ nhất (báo nhẹ cho người dùng).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct CacheTrimmed {
    pub files: u32,
    #[specta(type = f64)]
    pub bytes: u64,
}

pub struct JobsAppState {
    pub scheduler: JobScheduler,
}

impl Default for JobsAppState {
    fn default() -> Self {
        Self { scheduler: JobScheduler::new() }
    }
}

pub fn setup(app: &AppHandle) {
    // Lúc khởi động: cache có thể đã vượt giới hạn từ lần trước (hoặc giới hạn vừa bị hạ).
    {
        let h = app.clone();
        std::thread::spawn(move || enforce_cache_limit(&h));
    }
    let handle = app.clone();
    app.state::<JobsAppState>().scheduler.on_update(move |key, status| {
        JobsUpdate { media_id: key.to_string(), status: status.clone() }.emit(&handle).ok();
        if let JobStatus::Ready { .. } = status {
            let h = handle.clone();
            // Proxy mới → preview chuyển sang proxy; kiểm giới hạn cache.
            std::thread::spawn(move || {
                refresh_preview(&h);
                enforce_cache_limit(&h);
            });
        }
    });
}

/// K-S4: mọi trạng thái khác Idle (chuẩn bị, đếm ngược, đang quay, tạm dừng, đang lưu) → hoãn.
pub fn on_recorder_state(app: &AppHandle, state: RecorderState) {
    let jobs = app.state::<JobsAppState>();
    if state == RecorderState::Idle {
        jobs.scheduler.resume();
    } else {
        jobs.scheduler.suspend();
    }
}

/// Xếp việc tạo proxy cho các media nặng của project đang mở.
pub fn enqueue_for_open_project(app: &AppHandle) {
    let editor = app.state::<EditorAppState>();
    let jobs: Vec<ProxyJob> = editor
        .with_session(|s| {
            s.project()
                .media
                .iter()
                .filter(|m| mochi_jobs::needs_proxy(m))
                .filter_map(|m| {
                    s.media_path(&m.id).map(|source| ProxyJob {
                        key: m.id.clone(),
                        source,
                        target: mochi_jobs::proxy_path_for(s.dir(), m),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let scheduler = &app.state::<JobsAppState>().scheduler;
    for j in jobs {
        scheduler.push(j);
    }
}

/// Gửi danh sách proxy đang có cho trình phát (preview chuyển sang / bỏ proxy).
/// Người dùng tắt proxy (S03 "bật/tắt được") → preview đọc file gốc.
pub fn refresh_preview(app: &AppHandle) {
    let on = app.state::<crate::settings::SettingsState>().get().use_proxies;
    let ready = if on { app.state::<JobsAppState>().scheduler.ready() } else { HashMap::new() };
    app.state::<EditorAppState>().set_proxies(ready);
}

/// S03: bật / tắt dùng proxy cho preview (xuất luôn đọc file gốc).
#[tauri::command]
#[specta::specta]
pub fn preview_use_proxies(app: AppHandle, on: bool) -> Result<(), String> {
    app.state::<crate::settings::SettingsState>()
        .update(|s| s.use_proxies = on)
        .map_err(|e| e.to_string())?;
    refresh_preview(&app);
    Ok(())
}

fn limit_bytes(app: &AppHandle) -> u64 {
    let gb = app.state::<crate::settings::SettingsState>().get().cache_limit_gb;
    (gb.max(1) as u64) * 1024 * 1024 * 1024
}

/// Các gói project mà cache được tính chung: project đang mở + project gần đây.
fn project_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = app.state::<crate::settings::SettingsState>().get().recent_projects;
    if let Some(d) = app.state::<EditorAppState>().with_session(|s| s.dir().to_path_buf())
        && !dirs.contains(&d)
    {
        dirs.push(d);
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// Cache vừa thêm ảnh thu nhỏ / waveform → kiểm giới hạn (tối đa mỗi 30 s một lần, chạy nền).
pub fn cache_grew(app: &AppHandle) {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(30)) {
        return;
    }
    *last = Some(std::time::Instant::now());
    let h = app.clone();
    std::thread::spawn(move || enforce_cache_limit(&h));
}

/// Đổi giới hạn cache (CachePanel): lưu rồi dọn ngay nếu đang vượt.
#[tauri::command]
#[specta::specta]
pub async fn cache_set_limit(app: AppHandle, gb: u32) -> Result<CacheInfo, String> {
    app.state::<crate::settings::SettingsState>()
        .update(|s| s.cache_limit_gb = gb.clamp(1, 500))
        .map_err(|e| e.to_string())?;
    let h = app.clone();
    tokio::task::spawn_blocking(move || enforce_cache_limit(&h)).await.map_err(|e| e.to_string())?;
    Ok(cache_info(app).await)
}

fn enforce_cache_limit(app: &AppHandle) {
    let removed = cache::enforce_limit(&project_dirs(app), limit_bytes(app));
    if removed.is_empty() {
        return;
    }
    let keys: Vec<String> = removed
        .iter()
        .filter(|e| e.kind == "proxies")
        .filter_map(|e| e.path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    app.state::<JobsAppState>().scheduler.forget(&keys);
    let bytes = removed.iter().map(|e| e.bytes).sum();
    tracing::info!("cache vượt giới hạn: đã xóa {} file ({bytes} byte)", removed.len());
    CacheTrimmed { files: removed.len() as u32, bytes }.emit(app).ok();
    refresh_preview(app);
}

#[tauri::command]
#[specta::specta]
pub fn jobs_status(jobs: State<'_, JobsAppState>) -> HashMap<String, JobStatus> {
    jobs.scheduler.status()
}

/// Thử lại tác vụ nền đã lỗi của một media.
#[tauri::command]
#[specta::specta]
pub fn jobs_retry(app: AppHandle, media_id: String) {
    let job = app.state::<EditorAppState>().with_session(|s| {
        let item = s.project().media.iter().find(|m| m.id == media_id)?;
        s.media_path(&media_id).map(|source| ProxyJob {
            key: media_id.clone(),
            source,
            target: mochi_jobs::proxy_path_for(s.dir(), item),
        })
    });
    if let Some(Some(j)) = job {
        app.state::<JobsAppState>().scheduler.retry(j);
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub usage: CacheUsage,
    pub limit_gb: u32,
}

#[tauri::command]
#[specta::specta]
pub async fn cache_info(app: AppHandle) -> CacheInfo {
    let dirs = project_dirs(&app);
    let limit_gb = app.state::<crate::settings::SettingsState>().get().cache_limit_gb;
    let usage = tokio::task::spawn_blocking(move || cache::usage(&dirs)).await.unwrap_or_default();
    CacheInfo { usage, limit_gb }
}

/// Dọn toàn bộ cache (proxy, ảnh thu nhỏ, waveform) — K-S3: bản quay và media gốc không bị đụng.
#[tauri::command]
#[specta::specta]
pub async fn cache_clear(app: AppHandle) -> CacheInfo {
    let dirs = project_dirs(&app);
    let d2 = dirs.clone();
    let freed = tokio::task::spawn_blocking(move || cache::clear(&d2, &[])).await.unwrap_or(0);
    tracing::info!("dọn cache: giải phóng {freed} byte");
    let scheduler = &app.state::<JobsAppState>().scheduler;
    let keys: Vec<String> = scheduler.status().into_keys().collect();
    scheduler.forget(&keys);
    refresh_preview(&app);
    enqueue_for_open_project(&app);
    cache_info(app).await
}

/// Lỗi "GPU / driver không đạt yêu cầu tối thiểu": báo lúc khởi động, vẫn cho quay.
#[tauri::command]
#[specta::specta]
pub async fn system_gpu_check(app: AppHandle) -> mochi_render::GpuCheck {
    let check = tokio::task::spawn_blocking(move || app.state::<EditorAppState>().gpu_check())
        .await
        .unwrap_or_else(|e| mochi_render::GpuCheck { ok: false, adapter: e.to_string(), driver: String::new(), software: false, old_driver: false });
    tracing::info!(?check, "kiểm GPU");
    check
}
