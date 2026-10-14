use crate::settings::VideoCodec;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Lỗi xuất. Mỗi biến thể ứng với một luật K-X hoặc một ca lỗi trong use case.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, specta::Type)]
#[serde(tag = "code")]
pub enum ExportError {
    /// K-X1
    #[error("file đã tồn tại: {}", path.display())]
    #[serde(rename = "fileExists", rename_all = "camelCase")]
    FileExists { path: PathBuf },
    /// K-X2
    #[error("thiếu media: {}", ids.join(", "))]
    #[serde(rename = "missingMedia", rename_all = "camelCase")]
    MissingMedia { ids: Vec<String>, names: Vec<String> },
    /// K-X3
    #[error("máy không hỗ trợ encoder {codec:?}: {reason}")]
    #[serde(rename = "unsupportedEncoder", rename_all = "camelCase")]
    UnsupportedEncoder { codec: VideoCodec, reason: String },
    #[error("không ghi được vào thư mục {}", dir.display())]
    #[serde(rename = "notWritable", rename_all = "camelCase")]
    NotWritable { dir: PathBuf },
    /// Hết dung lượng (trước khi xuất hoặc giữa chừng).
    #[error("không đủ dung lượng: cần khoảng {needed_bytes} byte, còn {free_bytes}")]
    #[serde(rename = "diskFull", rename_all = "camelCase")]
    DiskFull {
        #[specta(type = f64)]
        needed_bytes: u64,
        #[specta(type = f64)]
        free_bytes: u64,
    },
    #[error("đoạn xuất không hợp lệ")]
    #[serde(rename = "invalidRange")]
    InvalidRange,
    #[error("project trống")]
    #[serde(rename = "emptyProject")]
    EmptyProject,
    #[error("encoder lỗi: {message}")]
    #[serde(rename = "encoder", rename_all = "camelCase")]
    Encoder { message: String, hardware: bool },
    #[error("đã hủy")]
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Tiến trình xuất chết giữa chừng (crash, bị kill).
    #[error("tiến trình xuất dừng bất thường (mã {exit_code:?}), xem log {}", log.display())]
    #[serde(rename = "crashed", rename_all = "camelCase")]
    Crashed { exit_code: Option<i32>, log: PathBuf },
    #[error("{message}")]
    #[serde(rename = "internal", rename_all = "camelCase")]
    Internal { message: String },
}

impl ExportError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal { message: e.to_string() }
    }
}
