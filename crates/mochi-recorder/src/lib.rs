//! mochi-recorder — dịch vụ quay của Mochi Studio (Người quay: R01–R08).
//!
//! Bọc pipeline studio của Cap (`cap-recording`) bằng máy trạng thái riêng để thực thi
//! các luật trong use-cases.md: K-R1 (một phiên), K-R2 (không đổi cấu hình khi quay),
//! K-R3 (ghi phím phải opt-in), K-R4 (dung lượng ổ).

mod activity;
pub mod app_audio;
mod config;
pub mod disk;
mod error;
mod events;
pub mod finalize;
mod follow;
pub mod meter;
pub mod recovery;
mod service;
pub mod targets;

pub use config::{CaptureTarget, KeystrokeMode, RecordingConfig, RecordingQuality};
pub use error::{DeviceKind, RecorderError};
pub use events::{
    FinishedRecording, RecorderEvent, RecorderState, RecorderWarning, RecordingStats,
};
pub use service::{FinishedPaths, RecorderService};
