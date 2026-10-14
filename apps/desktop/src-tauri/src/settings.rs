//! Thiết lập ứng dụng (plan mục 2.3): `<app_config_dir>/settings.json`.
//! Không có telemetry (K-S2).

use mochi_recorder::RecordingConfig;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    /// Bắt đầu quay / tạm dừng / tiếp tục (giống Camtasia F9)
    pub record_pause: String,
    /// Dừng (giống Camtasia F10)
    pub stop: String,
    /// Hủy bản quay đang quay
    pub cancel: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            record_pause: "F9".into(),
            stop: "F10".into(),
            cancel: "Shift+F10".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkeys: Hotkeys,
    /// Thư mục lưu bản quay; None = `Videos\Mochi Studio`.
    pub output_root: Option<PathBuf>,
    /// Cấu hình quay gần nhất (R11 ở v1 sẽ thành preset có tên).
    pub last_recording: Option<RecordingConfig>,
    pub countdown_secs: u32,
    /// Project mở gần đây (mới nhất trước).
    pub recent_projects: Vec<PathBuf>,
    /// Giới hạn cache (proxy, ảnh thu nhỏ, waveform), GB. Mặc định 20.
    pub cache_limit_gb: u32,
    /// S03: preview dùng proxy khi có (tắt → luôn đọc file gốc).
    pub use_proxies: bool,
    /// R11 (v1): cấu hình quay có tên.
    pub recording_presets: Vec<RecordingPreset>,
    /// E14 (v1): preset hiệu ứng bản quay (nền, con trỏ, webcam) có tên.
    pub fx_presets: Vec<FxPreset>,
    /// E14 (v1): theme thương hiệu — màu gợi ý cho ô màu, font mặc định cho chữ mới.
    pub brand: BrandTheme,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingPreset {
    pub name: String,
    pub config: RecordingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FxPreset {
    pub name: String,
    pub fx: mochi_project::RecordingFx,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct BrandTheme {
    /// "#RRGGBB", tối đa 8 màu.
    pub colors: Vec<String>,
    /// Họ font cho chữ mới (rỗng = font mặc định).
    pub font: String,
}

impl Default for BrandTheme {
    fn default() -> Self {
        Self {
            colors: ["#FFFFFF", "#1D1B26", "#E2557B", "#FFD54A", "#4FC3F7", "#2F9E6B"].map(String::from).to_vec(),
            font: String::new(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            output_root: None,
            last_recording: None,
            countdown_secs: 3,
            recent_projects: vec![],
            cache_limit_gb: 20,
            use_proxies: true,
            recording_presets: vec![],
            fx_presets: vec![],
            brand: BrandTheme::default(),
        }
    }
}

pub struct SettingsState {
    path: PathBuf,
    inner: Mutex<Settings>,
}

impl SettingsState {
    pub fn load(app: &AppHandle) -> anyhow::Result<Self> {
        let dir = app.path().app_config_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");
        let inner = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(Self {
            path,
            inner: Mutex::new(inner),
        })
    }

    pub fn get(&self) -> Settings {
        self.inner.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> anyhow::Result<Settings> {
        let mut guard = self.inner.lock().unwrap();
        f(&mut guard);
        // Ghi file tạm rồi đổi tên để không bao giờ để lại settings.json dở dang.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&*guard)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(guard.clone())
    }
}

pub fn default_output_root(app: &AppHandle) -> PathBuf {
    app.path()
        .video_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("Mochi Studio")
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(app: AppHandle, state: State<'_, SettingsState>) -> Settings {
    let mut s = state.get();
    if s.output_root.is_none() {
        s.output_root = Some(default_output_root(&app));
    }
    s
}

#[tauri::command]
#[specta::specta]
pub fn settings_set(state: State<'_, SettingsState>, settings: Settings) -> Result<Settings, String> {
    state
        .update(|s| *s = settings)
        .map_err(|e| e.to_string())
}
