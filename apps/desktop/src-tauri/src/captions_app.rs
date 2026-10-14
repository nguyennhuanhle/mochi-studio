//! E13 / S07 / X05 (v1): phụ đề local — model whisper (tải khi người dùng đồng ý), phiên âm nền,
//! nhập / xuất SRT, VTT. Kết quả trả về UI để áp bằng op `setCaptions` (hoàn tác được).

use crate::editor_app::EditorAppState;
use mochi_project::{Caption, Ticks};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

/// Tiến độ tải model / phiên âm (0–100).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct CaptionsProgress {
    /// "download" | "audio" | "transcribe"
    pub stage: String,
    pub pct: f32,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CaptionModel {
    pub id: String,
    #[specta(type = f64)]
    pub bytes: u64,
    pub installed: bool,
}

static CANCEL: std::sync::LazyLock<Arc<AtomicBool>> = std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));
/// Luật Hệ thống: không chạy phiên âm trong lúc quay → bật khi đang quay, phiên âm đứng chờ.
static RECORDING: std::sync::LazyLock<Arc<AtomicBool>> = std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));

pub fn on_recorder_state(state: mochi_recorder::RecorderState) {
    RECORDING.store(state != mochi_recorder::RecorderState::Idle, Ordering::Relaxed);
}

fn models_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir()).join("models")
}

#[tauri::command]
#[specta::specta]
pub fn captions_models(app: AppHandle) -> Vec<CaptionModel> {
    let dir = models_dir(&app);
    mochi_captions::MODELS
        .iter()
        .map(|m| CaptionModel { id: m.id.into(), bytes: m.bytes, installed: mochi_captions::is_installed(&dir, m) })
        .collect()
}

/// CPU có chạy được whisper.cpp không (AVX2 / FMA / F16C).
#[tauri::command]
#[specta::specta]
pub fn captions_cpu_check() -> Result<(), String> {
    mochi_captions::cpu_supported()
}

#[tauri::command]
#[specta::specta]
pub fn captions_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Tải model (người dùng đã đồng ý ở UI, biết trước dung lượng).
#[tauri::command]
#[specta::specta]
pub async fn captions_download(app: AppHandle, model: String) -> Result<(), String> {
    let m = mochi_captions::model(&model).ok_or("model không hợp lệ")?;
    let dir = models_dir(&app);
    CANCEL.store(false, Ordering::Relaxed);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        mochi_captions::download(&dir, &m, CANCEL.clone(), |done, total| {
            let _ = CaptionsProgress { stage: "download".into(), pct: (done as f64 / total.max(1) as f64 * 100.0) as f32 }.emit(&handle);
        })
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Phiên âm toàn bộ tiếng của timeline (đúng bộ trộn của preview / xuất) → phụ đề theo thời gian timeline.
#[tauri::command]
#[specta::specta]
pub async fn captions_generate(app: AppHandle, model: String, language: Option<String>) -> Result<Vec<Caption>, String> {
    let m = mochi_captions::model(&model).ok_or("model không hợp lệ")?;
    let dir = models_dir(&app);
    if !mochi_captions::is_installed(&dir, &m) {
        return Err("model chưa được tải".into());
    }
    mochi_captions::cpu_supported()?;
    let state = app.state::<EditorAppState>();
    let (project, paths) = state
        .with_session(|s| {
            let p = s.project().clone();
            let paths = p.media.iter().filter_map(|x| s.media_path(&x.id).map(|path| (x.id.clone(), path))).collect();
            (p, paths)
        })
        .ok_or("chưa mở project")?;
    CANCEL.store(false, Ordering::Relaxed);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |stage: &str, pct: f32| {
            let _ = CaptionsProgress { stage: stage.into(), pct }.emit(&handle);
        };
        emit("audio", 0.0);
        let end = project.clips.iter().map(|c| c.end()).max().unwrap_or(Ticks::ZERO);
        let mixer = mochi_preview::OfflineMixer::new(Arc::new(project), &paths);
        let rate = mochi_preview::OfflineMixer::RATE as i64;
        let total = (end.0 / mochi_preview::OfflineMixer::TICKS_PER_SAMPLE).max(0) as usize;
        let mut pcm48 = vec![0f32; total * 2];
        // Trộn từng đoạn 10 s (báo tiến độ, cho hủy).
        let chunk = (rate * 10) as usize;
        let mut at = 0usize;
        while at < total {
            if CANCEL.load(Ordering::Relaxed) {
                return Err("đã hủy".to_string());
            }
            if RECORDING.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            let n = chunk.min(total - at);
            mixer.fill(Ticks(at as i64 * mochi_preview::OfflineMixer::TICKS_PER_SAMPLE), &mut pcm48[at * 2..(at + n) * 2]);
            at += n;
            emit("audio", at as f32 / total.max(1) as f32 * 100.0);
        }
        let pcm16 = mochi_captions::to_16k_mono(&pcm48);
        drop(pcm48);
        let h2 = handle.clone();
        mochi_captions::transcribe(
            &mochi_captions::model_path(&dir, &m),
            &pcm16,
            language.as_deref().filter(|l| !l.is_empty() && *l != "auto"),
            Ticks::ZERO,
            CANCEL.clone(),
            RECORDING.clone(),
            move |p| {
                let _ = CaptionsProgress { stage: "transcribe".into(), pct: p as f32 }.emit(&h2);
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Xuất phụ đề của project ra .srt / .vtt (hộp thoại lưu). None = người dùng hủy.
#[tauri::command]
#[specta::specta]
pub async fn captions_export(app: AppHandle, vtt: bool) -> Result<Option<PathBuf>, String> {
    let state = app.state::<EditorAppState>();
    let (caps, name) = state.with_session(|s| (s.project().captions.clone(), s.project().name.clone())).ok_or("chưa mở project")?;
    let (label, ext) = if vtt { ("WebVTT", "vtt") } else { ("SubRip", "srt") };
    let Some(path) = app
        .dialog()
        .file()
        .add_filter(label, &[ext])
        .set_file_name(format!("{name}.{ext}"))
        .blocking_save_file()
        .and_then(|f| f.into_path().ok())
    else {
        return Ok(None);
    };
    let text = if vtt { mochi_captions::subtitles::to_vtt(&caps) } else { mochi_captions::subtitles::to_srt(&caps) };
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

/// Nhập .srt / .vtt (hộp thoại mở). None = người dùng hủy.
#[tauri::command]
#[specta::specta]
pub async fn captions_import(app: AppHandle) -> Result<Option<Vec<Caption>>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("SRT / VTT", &["srt", "vtt"])
        .blocking_pick_file()
        .and_then(|f| f.into_path().ok())
    else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let caps = mochi_captions::subtitles::parse(&text);
    if caps.is_empty() {
        return Err("không đọc được dòng phụ đề nào".into());
    }
    Ok(Some(caps))
}
