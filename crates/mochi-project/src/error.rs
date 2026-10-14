use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: PathBuf,
    /// RFC 3339
    pub saved_at: String,
    /// Bản người dùng tự lưu (Ctrl+S / đóng project), không phải bản tự lưu nền.
    pub manual: bool,
}

/// Lỗi khi làm việc với gói project. `code` ổn định để UI dịch.
#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum ProjectError {
    #[error("không thấy project")]
    NotFound,
    /// K-E3: project đang mở ở nơi khác.
    #[error("project đang được mở ở nơi khác")]
    Locked,
    /// K-E6: project tạo bởi phiên bản app mới hơn.
    #[error("project cần phiên bản app mới hơn")]
    #[serde(rename_all = "camelCase")]
    NewerSchema { found: u32, supported: u32 },
    /// project.json hỏng → đề nghị mở backup.
    #[error("project.json bị hỏng: {detail}")]
    Corrupt { detail: String, backups: Vec<BackupInfo> },
    /// K-E1: từ chối ghi ra ngoài các vị trí cho phép trong gói.
    #[error("từ chối ghi {path}")]
    WriteOutsideProject { path: String },
    #[error("file không hỗ trợ: {detail}")]
    Unsupported { detail: String },
    #[error("lỗi đọc/ghi: {detail}")]
    Io { detail: String },
}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        Self::Io { detail: e.to_string() }
    }
}
