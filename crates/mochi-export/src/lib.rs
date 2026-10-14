//! Người xuất bản (X01–X04, S05, K-X1…K-X4).
//!
//! - `preflight`: kiểm tra + ước tính, chạy ở tiến trình cha.
//! - `caps`: encoder máy hỗ trợ (tự kiểm thật).
//! - `job`: render → encode, chạy ở tiến trình con (`child`).
//! - `queue`: mỗi project một lần xuất chạy cùng lúc.

pub mod audio_enc;
pub mod caps;
pub mod child;
pub mod encode;
pub mod error;
pub mod job;
pub mod preflight;
pub mod queue;
pub mod settings;

pub use caps::{CodecSupport, encoder_capabilities};
pub use child::{ChildExport, child_main, job_arg};
pub use error::ExportError;
pub use job::{ChildMsg, ExportJob};
pub use preflight::{ExportEstimate, ExportWarning, preflight};
pub use queue::{ExportQueue, JobState};
pub use settings::{ExportFormat, ExportQuality, ExportSettings, VideoCodec};
