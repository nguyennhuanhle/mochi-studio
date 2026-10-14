use serde::Serialize;

/// Lỗi của dịch vụ quay. `code` ổn định để UI dịch sang vi/en (không hiển thị chuỗi Rust thô).
#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum RecorderError {
    /// K-R1: đang có một phiên quay.
    #[error("đang có phiên quay khác")]
    AlreadyRecording,
    /// K-R2: chỉ được mute/unmute khi đang quay.
    #[error("không đổi được cấu hình khi đang quay")]
    NotIdle,
    #[error("không có phiên quay nào")]
    NotRecording,
    /// K-R4: ổ đích còn ít hơn ngưỡng.
    #[error("ổ đĩa còn {free_bytes} byte, cần {required_bytes}")]
    #[serde(rename_all = "camelCase")]
    LowDisk { free_bytes: f64, required_bytes: f64 },
    #[error("không thấy vùng quay")]
    TargetNotFound,
    #[error("không thấy mic {name}")]
    MicNotFound { name: String },
    #[error("không thấy webcam {id}")]
    CameraNotFound { id: String },
    /// Hệ điều hành chặn quyền mic/webcam.
    #[error("chưa cấp quyền {kind}")]
    PermissionDenied { kind: DeviceKind },
    #[error("thiết bị {kind} lỗi: {detail}")]
    DeviceFailed { kind: DeviceKind, detail: String },
    #[error("không bắt đầu quay được: {detail}")]
    StartFailed { detail: String },
    #[error("lỗi nội bộ: {detail}")]
    Internal { detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Microphone,
    Camera,
    SystemAudio,
    Screen,
}

impl RecorderError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal { detail: e.to_string() }
    }
}

impl std::fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Microphone => "mic",
            Self::Camera => "webcam",
            Self::SystemAudio => "âm thanh hệ thống",
            Self::Screen => "màn hình",
        })
    }
}
