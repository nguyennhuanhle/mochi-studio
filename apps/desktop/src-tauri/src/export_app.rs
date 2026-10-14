//! Người xuất bản (X01–X04): lệnh xuất của app.
//!
//! Preflight chạy ở đây (nhanh); render + encode chạy ở tiến trình con là chính file exe này với
//! `--export-job` (xem `main.rs`), nên encoder / driver có crash thì editor vẫn sống (X04).
//! Job chụp lại project lúc bấm xuất → người dùng sửa tiếp không ảnh hưởng file đang xuất.

use crate::editor_app::EditorAppState;
use mochi_export::{
    ChildExport, ChildMsg, CodecSupport, ExportError, ExportEstimate, ExportJob, ExportQueue, ExportSettings, JobState,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

/// Mọi thay đổi trạng thái của một job xuất.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ExportEvent {
    pub job_id: String,
    pub msg: ExportEventMsg,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExportEventMsg {
    /// Job rời hàng đợi, bắt đầu chạy.
    Running,
    /// Tin nhắn từ tiến trình con (started / progress / fallback / done / failed).
    Child { msg: ChildMsg },
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportJobInfo {
    pub job_id: String,
    pub state: JobState,
    pub output: PathBuf,
    pub project_name: String,
}

#[derive(Default)]
pub struct ExportAppState {
    queue: Mutex<ExportQueue<ExportJob>>,
    running: Mutex<HashMap<String, Arc<ChildExport>>>,
}

fn project_key(job: &ExportJob) -> String {
    job.project.id.clone()
}

/// Project đang mở + đường dẫn các media đang có trên đĩa.
fn snapshot(state: &EditorAppState) -> Result<(mochi_project::Project, HashMap<String, PathBuf>), ExportError> {
    state
        .with_session(|s| {
            let paths = s
                .project()
                .media
                .iter()
                .filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p)))
                .collect();
            (s.project().clone(), paths)
        })
        .ok_or_else(|| ExportError::internal("chưa mở project"))
}

/// Kiểm encoder ngay khi mở app (mất ~2 s, chạy nền) để hộp thoại Xuất mở ra là có kết quả.
pub fn setup() {
    std::thread::Builder::new()
        .name("mochi-encoder-caps".into())
        .spawn(|| {
            let caps = mochi_export::encoder_capabilities();
            tracing::info!("encoder: {}", caps.iter().map(|c| format!("{:?}={:?}", c.codec, c.encoder)).collect::<Vec<_>>().join(", "));
        })
        .ok();
}

#[tauri::command]
#[specta::specta]
pub async fn export_capabilities() -> Vec<CodecSupport> {
    tokio::task::spawn_blocking(|| mochi_export::encoder_capabilities().to_vec()).await.unwrap_or_default()
}

/// Kiểm tra + ước tính (gọi mỗi khi đổi thiết lập trong hộp thoại Xuất).
#[tauri::command]
#[specta::specta]
pub async fn export_estimate(
    editor: State<'_, EditorAppState>,
    settings: ExportSettings,
) -> Result<ExportEstimate, ExportError> {
    let (project, paths) = snapshot(&editor)?;
    tokio::task::spawn_blocking(move || mochi_export::preflight(&project, &paths, &settings))
        .await
        .map_err(ExportError::internal)?
}

#[tauri::command]
#[specta::specta]
pub async fn export_start(
    app: AppHandle,
    editor: State<'_, EditorAppState>,
    exports: State<'_, ExportAppState>,
    settings: ExportSettings,
) -> Result<ExportJobInfo, ExportError> {
    let (project, paths) = snapshot(&editor)?;
    enqueue(&app, &exports, project, paths, settings).await
}

/// X06 (v1): xuất một project KHÁC project đang mở (xuất hàng loạt): mở gói ở chế độ đọc (giữ khóa
/// trong lúc chụp), chụp project + đường dẫn media rồi đóng ngay; job chạy như mọi job xuất khác.
#[tauri::command]
#[specta::specta]
pub async fn export_project_file(
    app: AppHandle,
    exports: State<'_, ExportAppState>,
    dir: PathBuf,
    settings: ExportSettings,
) -> Result<ExportJobInfo, ExportError> {
    let (project, paths) = tokio::task::spawn_blocking(move || {
        let s = mochi_editor::EditorSession::open(&dir).map_err(|e| ExportError::internal(e.to_string()))?;
        let p = s.project().clone();
        let paths: HashMap<String, PathBuf> = p.media.iter().filter_map(|m| s.media_path(&m.id).map(|x| (m.id.clone(), x))).collect();
        Ok::<_, ExportError>((p, paths))
    })
    .await
    .map_err(ExportError::internal)??;
    enqueue(&app, &exports, project, paths, settings).await
}

async fn enqueue(
    app: &AppHandle,
    exports: &ExportAppState,
    project: mochi_project::Project,
    paths: HashMap<String, PathBuf>,
    settings: ExportSettings,
) -> Result<ExportJobInfo, ExportError> {
    let (project, paths, settings, est) = tokio::task::spawn_blocking(move || {
        mochi_export::preflight(&project, &paths, &settings).map(|est| (project, paths, settings, est))
    })
    .await
    .map_err(ExportError::internal)??;

    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let log_dir = app.path().app_log_dir().map_err(ExportError::internal)?;
    let project_name = project.name.clone();
    let job = ExportJob {
        id: id.clone(),
        project,
        paths,
        settings,
        output: est.output.clone(),
        log: log_dir.join(format!("export-{id}.log")),
    };
    let key = project_key(&job);
    let (state, to_run) = exports.queue.lock().unwrap().push(&key, &id, job);
    if let Some(job) = to_run {
        launch(app, job);
    }
    tracing::info!("job xuất {id} → {} ({state:?})", est.output.display());
    Ok(ExportJobInfo { job_id: id, state, output: est.output, project_name })
}

/// Chạy tiến trình con cho `job`; khi xong thì chạy job kế tiếp cùng project (K-X4).
fn launch(app: &AppHandle, job: ExportJob) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            finish(app, &job, Err(ExportError::internal(e)));
            return;
        }
    };
    let id = job.id.clone();
    ExportEvent { job_id: id.clone(), msg: ExportEventMsg::Running }.emit(app).ok();
    let handle = app.clone();
    let msg_id = id.clone();
    let child = ChildExport::spawn(&exe, &[], &job, move |msg| {
        ExportEvent { job_id: msg_id.clone(), msg: ExportEventMsg::Child { msg } }.emit(&handle).ok();
    });
    let child = match child {
        Ok(c) => Arc::new(c),
        Err(e) => {
            finish(app, &job, Err(e));
            return;
        }
    };
    app.state::<ExportAppState>().running.lock().unwrap().insert(id, child.clone());
    let handle = app.clone();
    std::thread::spawn(move || {
        let result = child.wait();
        finish(&handle, &job, result);
    });
}

fn finish(app: &AppHandle, job: &ExportJob, result: Result<ChildMsg, ExportError>) {
    let msg = match result {
        Ok(m) => m,
        Err(error) => ChildMsg::Failed { error },
    };
    tracing::info!("job xuất {} kết thúc: {msg:?}", job.id);
    ExportEvent { job_id: job.id.clone(), msg: ExportEventMsg::Child { msg } }.emit(app).ok();
    let state = app.state::<ExportAppState>();
    state.running.lock().unwrap().remove(&job.id);
    let next = state.queue.lock().unwrap().finish(&project_key(job), &job.id);
    if let Some((_, next)) = next {
        launch(app, next);
    }
}

/// X04: hủy job đang chạy (tiến trình con tự xóa file tạm) hoặc bỏ job đang chờ.
#[tauri::command]
#[specta::specta]
pub fn export_cancel(app: AppHandle, exports: State<'_, ExportAppState>, job_id: String) {
    if let Some(child) = exports.running.lock().unwrap().get(&job_id) {
        child.cancel();
        return;
    }
    if exports.queue.lock().unwrap().remove_waiting(&job_id) {
        ExportEvent { job_id, msg: ExportEventMsg::Child { msg: ChildMsg::Failed { error: ExportError::Cancelled } } }
            .emit(&app)
            .ok();
    }
}

/// Hộp thoại chọn nơi lưu file xuất.
#[tauri::command]
#[specta::specta]
pub async fn export_pick_path(app: AppHandle, current: PathBuf, format: mochi_export::ExportFormat) -> Option<PathBuf> {
    use mochi_export::ExportFormat;
    let (label, ext) = match format {
        ExportFormat::Gif => ("GIF", "gif"),
        ExportFormat::M4a => ("M4A", "m4a"),
        ExportFormat::Mp3 => ("MP3", "mp3"),
        ExportFormat::Webm => ("WebM", "webm"),
        ExportFormat::Mp4 => ("MP4", "mp4"),
    };
    let mut dialog = app.dialog().file().add_filter(label, &[ext]);
    if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = current.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    dialog.blocking_save_file().and_then(|f| f.into_path().ok())
}

/// S08 (v1): kiểm tra phiên bản mới = mở trang Releases trên trình duyệt. App KHÔNG tự gửi request
/// (K-S1: không kết nối mạng); trình duyệt của người dùng làm việc đó khi họ bấm.
pub const RELEASES_URL: &str = "https://github.com/nguyennhuanhle/mochi-studio/releases";

#[tauri::command]
#[specta::specta]
pub fn open_releases_page(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(RELEASES_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Phiên bản app đang chạy (để so với trang Releases).
#[tauri::command]
#[specta::specta]
pub fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Thư mục mặc định cho file xuất: thư mục lưu bản quay (Videos\Mochi Studio).
#[tauri::command]
#[specta::specta]
pub fn export_default_dir(app: AppHandle) -> PathBuf {
    let settings = app.state::<crate::settings::SettingsState>();
    crate::recorder_app::output_root(&app, &settings)
}

/// Mở file vừa xuất bằng app mặc định của máy.
#[tauri::command]
#[specta::specta]
pub fn export_open_file(app: AppHandle, path: PathBuf) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}
