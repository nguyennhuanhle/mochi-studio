//! Dữ liệu riêng của Mochi đi kèm một bản quay (`recordings/<id>/mochi-recording.json`):
//! khoảng mất thiết bị (R07 — đánh dấu trên timeline) và cửa sổ đang active (R03).
//! Thời gian tính bằng giây trên bản quay (đã trừ các lần tạm dừng, khớp với các segment nối liền).

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SIDECAR_FILE: &str = "mochi-recording.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LostSource {
    Camera,
    Microphone,
    SystemAudio,
}

/// Một nguồn bị mất trong [start, end) giây.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LostRange {
    pub source: LostSource,
    pub start: f64,
    pub end: f64,
}

/// Cửa sổ đang active đổi sang `app` / `title` tại giây `at`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindow {
    pub at: f64,
    pub app: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordingSidecar {
    pub lost: Vec<LostRange>,
    pub active_windows: Vec<ActiveWindow>,
    /// R13: highlight con trỏ đã chọn ở màn thiết lập quay → mặc định cho clip của bản quay này.
    pub cursor_highlight: Option<crate::CursorHighlight>,
}

impl RecordingSidecar {
    /// Không có file / file hỏng → rỗng (dữ liệu phụ, không chặn việc mở bản quay).
    pub fn read(recording_dir: &Path) -> Self {
        std::fs::read(recording_dir.join(SIDECAR_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, recording_dir: &Path) -> std::io::Result<()> {
        let tmp = recording_dir.join(format!("{SIDECAR_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, recording_dir.join(SIDECAR_FILE))
    }
}
