use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Vùng quay (R01). Toạ độ `Area` là toạ độ LOGIC trên màn hình `display_id`, giống Cap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CaptureTarget {
    Display {
        id: String,
    },
    Window {
        id: String,
    },
    Area {
        #[serde(rename = "displayId")]
        display_id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

/// K-R3: mặc định không ghi phím. `ModifiersOnly` chỉ giữ tổ hợp có Ctrl/Alt/Win;
/// `All` ghi mọi phím và chỉ bật được sau hộp thoại cảnh báo ở UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KeystrokeMode {
    #[default]
    Off,
    ModifiersOnly,
    All,
}

/// Mức chất lượng (bitrate) của bản quay. Chế độ studio luôn quay ở độ phân giải gốc
/// để zoom vẫn nét; độ phân giải đầu ra chọn lúc export (X01).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum RecordingQuality {
    Standard,
    #[default]
    High,
    Max,
}

impl From<RecordingQuality> for cap_recording::StudioQuality {
    fn from(q: RecordingQuality) -> Self {
        match q {
            RecordingQuality::Standard => Self::Compatibility,
            RecordingQuality::High => Self::Balanced,
            RecordingQuality::Max => Self::Ultra,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingConfig {
    pub target: CaptureTarget,
    /// Tên mic (như trong danh sách thiết bị); None = không thu mic.
    pub mic: Option<String>,
    /// id webcam; None = không quay cam.
    pub camera: Option<String>,
    pub system_audio: bool,
    /// 30 hoặc 60 (R06).
    pub fps: u32,
    pub quality: RecordingQuality,
    /// Đếm ngược trước khi quay, 0 = tắt (R04).
    pub countdown_secs: u32,
    pub keystrokes: KeystrokeMode,
    /// Thư mục gốc chứa các bản quay (mỗi bản một thư mục con).
    pub output_root: PathBuf,
    /// R13: highlight con trỏ cho bản quay này (không vẽ lên màn hình lúc quay; thành mặc định
    /// của clip khi mở trong editor).
    #[serde(default)]
    pub cursor_highlight: mochi_project::CursorHighlight,
    /// R08: thêm nền & khung (auto-polish) cho clip mới. Tắt = xuất trọn khung hình đã quay.
    #[serde(default = "yes")]
    pub frame: bool,
}

fn yes() -> bool {
    true
}

impl RecordingConfig {
    pub fn clamp(mut self) -> Self {
        self.fps = if self.fps >= 45 { 60 } else { 30 };
        self.countdown_secs = self.countdown_secs.min(10);
        self
    }
}
