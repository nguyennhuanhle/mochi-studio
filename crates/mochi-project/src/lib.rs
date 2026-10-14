//! mochi-project — gói project `.mochi` của Mochi Studio (Người biên tập: E01, S01, K-E1, K-E3, K-E6).

pub mod autozoom;
mod error;
pub mod media;
mod model;
pub mod package;
pub mod sidecar;
pub mod bundle;

pub use error::{BackupInfo, ProjectError};
pub use model::*;
pub use package::{BackupKind, ProjectFs, ProjectPackage};

/// Biến môi trường kiểm thử / dev (`MOCHI_TEST_*`, `MOCHI_UI_SNAPSHOT`, `MOCHI_PREVIEW_SILENT`).
/// Chỉ có tác dụng ở bản dev (`debug_assertions`); bản release (installer) luôn trả về None —
/// không ai hạ được ngưỡng ổ đĩa, ép lỗi encoder… trên máy người dùng bằng biến môi trường.
pub fn test_env(name: &str) -> Option<String> {
    if cfg!(debug_assertions) { std::env::var(name).ok() } else { None }
}

/// Project rỗng với một track hình và một track tiếng.
pub fn new_project(name: &str) -> Project {
    let now = chrono::Local::now().to_rfc3339();
    Project {
        schema_version: SCHEMA_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        canvas: Canvas::default(),
        media: vec![],
        tracks: vec![
            Track {
                id: uuid::Uuid::new_v4().to_string(),
                kind: TrackKind::Video,
                name: "Video 1".into(),
                locked: false,
                muted: false,
                hidden: false,
                volume_db: 0.0,
                magnetic: false,
                duck: false,
            },
            Track {
                id: uuid::Uuid::new_v4().to_string(),
                kind: TrackKind::Audio,
                name: "Audio 1".into(),
                locked: false,
                muted: false,
                hidden: false,
                volume_db: 0.0,
                magnetic: false,
                duck: false,
            },
        ],
        clips: vec![],
        markers: vec![],
        captions: vec![],
        caption_style: Default::default(),
    }
}
