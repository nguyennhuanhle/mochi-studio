//! Thiết lập xuất (X01–X03) và các phép tính dùng chung: kích thước khung, số frame, dung lượng.

use mochi_project::{Project, Ticks, TimeRange, TICKS_PER_SECOND};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Mp4,
    Gif,
    /// X02 (v1): chỉ âm thanh, AAC trong .m4a.
    #[serde(rename = "m4a")]
    M4a,
    /// X02 (v1): chỉ âm thanh, MP3 (LAME).
    Mp3,
}

impl ExportFormat {
    pub fn is_audio_only(self) -> bool {
        matches!(self, Self::M4a | Self::Mp3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 3] = [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportQuality {
    Standard,
    High,
    Max,
}

impl ExportQuality {
    /// Bit mỗi điểm ảnh mỗi frame (H.264). HEVC / AV1 nén tốt hơn nên dùng ít hơn.
    pub fn bpp(self, codec: VideoCodec) -> f32 {
        let base = match self {
            Self::Standard => 0.08,
            Self::High => 0.15,
            Self::Max => 0.3,
        };
        match codec {
            VideoCodec::H264 => base,
            VideoCodec::Hevc => base * 0.7,
            VideoCodec::Av1 => base * 0.6,
        }
    }

    /// Chất lượng gifski (1–100).
    pub fn gif_quality(self) -> u8 {
        match self {
            Self::Standard => 70,
            Self::High => 85,
            Self::Max => 95,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    pub format: ExportFormat,
    /// Bỏ qua khi xuất GIF.
    pub codec: VideoCodec,
    /// Cạnh ngắn của khung xuất (720, 1080, 2160…). Cạnh dài tính theo tỉ lệ canvas.
    pub short_side: u32,
    pub fps: u32,
    pub quality: ExportQuality,
    /// X03: chỉ xuất đoạn [start, end) của timeline. `None` = cả project.
    pub range: Option<TimeRange>,
    pub output: PathBuf,
    /// K-X1: chỉ ghi đè khi người dùng đã xác nhận.
    pub overwrite: bool,
    /// K-X2: chấp nhận thay đoạn thiếu media bằng khung trống.
    pub allow_black_fill: bool,
}

/// Số chẵn gần nhất (encoder YUV 4:2:0 cần kích thước chẵn).
fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32 * 2).max(2)
}

/// Kích thước khung xuất: giữ tỉ lệ canvas, cạnh ngắn = `short_side`.
pub fn output_size(project: &Project, short_side: u32) -> (u32, u32) {
    let (cw, ch) = (project.canvas.width.max(2) as f64, project.canvas.height.max(2) as f64);
    let s = short_side.max(2) as f64;
    if cw >= ch {
        (even(s * cw / ch), even(s))
    } else {
        (even(s), even(s * ch / cw))
    }
}

pub fn duration_of(project: &Project) -> Ticks {
    project.clips.iter().map(|c| c.end()).max().unwrap_or(Ticks::ZERO)
}

/// Đoạn timeline sẽ xuất.
pub fn export_range(project: &Project, s: &ExportSettings) -> TimeRange {
    s.range.unwrap_or(TimeRange { start: Ticks::ZERO, end: duration_of(project) })
}

/// Số frame của đoạn xuất (làm tròn lên: frame cuối vẫn có hình).
pub fn frame_count(range: &TimeRange, fps: u32) -> u32 {
    let len = (range.end.0 - range.start.0).max(0) as i128;
    let fps = fps.max(1) as i128;
    ((len * fps + TICKS_PER_SECOND as i128 - 1) / TICKS_PER_SECOND as i128) as u32
}

/// Thời điểm (tick timeline) của frame `i`. 705.600.000 chia hết cho 24/25/30/48/50/60 → không trôi.
pub fn frame_time(range: &TimeRange, fps: u32, i: u32) -> Ticks {
    Ticks(range.start.0 + i as i64 * TICKS_PER_SECOND / fps.max(1) as i64)
}

/// Bitrate video (bit/s) theo chất lượng.
pub fn video_bitrate(w: u32, h: u32, fps: u32, quality: ExportQuality, codec: VideoCodec) -> u64 {
    (w as f64 * h as f64 * fps as f64 * quality.bpp(codec) as f64) as u64
}

pub const AUDIO_BITRATE: u64 = 320_000;

/// File tạm trong lúc xuất, cùng thư mục đích: `Tên.mochi-partial.mp4`. Xong thì đổi tên.
pub fn partial_path(output: &Path) -> PathBuf {
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = output.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    output.with_file_name(format!("{stem}.mochi-partial.{ext}"))
}

/// Đuôi file đúng với định dạng.
pub fn with_format_extension(output: &Path, format: ExportFormat) -> PathBuf {
    output.with_extension(match format {
        ExportFormat::Mp4 => "mp4",
        ExportFormat::Gif => "gif",
        ExportFormat::M4a => "m4a",
        ExportFormat::Mp3 => "mp3",
    })
}
