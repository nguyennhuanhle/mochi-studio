use crate::{DeviceKind, RecorderError};
use serde::Serialize;
use std::path::PathBuf;

/// Máy trạng thái của Người quay (plan mục 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum RecorderState {
    Idle,
    /// Đang kiểm tra trước khi quay và khởi động pipeline.
    Preparing,
    Countdown,
    Recording,
    Paused,
    Finalizing,
}

impl RecorderState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Countdown | Self::Recording | Self::Paused)
    }
}

/// Số liệu gửi mỗi giây khi đang quay (R07). Số lớn dùng f64 vì TS không có u64.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStats {
    pub elapsed_ms: f64,
    pub bytes: f64,
    pub free_bytes: f64,
    pub segments: u32,
    pub mic_muted: bool,
}

/// Cảnh báo trong lúc quay. `code` ổn định để UI dịch.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum RecorderWarning {
    /// Thiết bị bị rút; các nguồn khác vẫn quay tiếp.
    DeviceLost { kind: DeviceKind },
    DeviceRestored { kind: DeviceKind },
    #[serde(rename_all = "camelCase")]
    DiskLow { free_bytes: f64 },
    /// Ổ gần đầy → đã tự dừng và giữ phần đã quay.
    StoppedDiskFull,
    /// Cửa sổ đang quay bị đóng → đã tự tạm dừng.
    WindowClosed,
    /// Cửa sổ đang quay bị thu nhỏ → đã tự tạm dừng.
    WindowMinimized,
    /// Màn hình đổi độ phân giải/DPI → đã đóng segment và quay tiếp ở segment mới.
    DisplayChanged,
    /// Màn hình đang quay bị rút → đã tự tạm dừng.
    DisplayLost,
    /// Đổi độ phân giải / DPI làm vùng quay nằm ngoài màn hình → đã tự tạm dừng.
    AreaOutOfBounds,
    #[serde(rename_all = "camelCase")]
    FrameDrops { source: String, rate_pct: f64 },
    EncoderFallback { backend: String },
    #[serde(rename_all = "camelCase")]
    AudioGap { gap_ms: f64 },
    /// Pipeline lỗi giữa chừng → đã tự dừng và lưu phần đã quay.
    PipelineFailed { detail: String },
    /// R10 (v1): không thu riêng được tiếng app (Windows quá cũ / app không cho) → thu âm thanh hệ thống.
    AppAudioUnavailable,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FinishedRecording {
    /// Thư mục gói project `Tên.mochi`.
    pub project_dir: PathBuf,
    /// Thư mục bản quay theo layout Cap, trong `recordings/`.
    pub recording_dir: PathBuf,
    pub name: String,
    pub duration_ms: f64,
    pub segments: u32,
    /// Ngắn hơn 1 giây → UI hỏi giữ hay bỏ.
    pub too_short: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RecorderEvent {
    StateChanged { state: RecorderState },
    Countdown { remaining: u32 },
    Stats { stats: RecordingStats },
    Warning { warning: RecorderWarning },
    Finished { recording: FinishedRecording },
    Failed { error: RecorderError },
}
