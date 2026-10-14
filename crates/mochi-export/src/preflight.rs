//! Kiểm tra trước khi xuất + ước tính. Chạy ở tiến trình cha (nhanh, không mở GPU).

use crate::caps;
use crate::error::ExportError;
use crate::settings::*;
use mochi_project::{ClipContent, Project};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};

/// GIF ước tính trên mức này thì cảnh báo trước.
pub const GIF_WARN_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportWarning {
    /// GIF ước tính quá lớn: gợi ý giảm fps / kích thước / độ dài.
    #[serde(rename_all = "camelCase")]
    GifTooLarge {
        #[specta(type = f64)]
        bytes: u64,
    },
    /// Tên file có ký tự không hợp lệ đã được thay.
    #[serde(rename_all = "camelCase")]
    FileNameChanged { from: String, to: String },
    /// K-X2: có media thiếu, người dùng đã chấp nhận khung trống.
    #[serde(rename_all = "camelCase")]
    BlackFill { names: Vec<String> },
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportEstimate {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub duration_secs: f64,
    #[specta(type = f64)]
    pub bytes: u64,
    /// Đường dẫn cuối cùng (đã sửa tên + đuôi file).
    pub output: PathBuf,
    /// Encoder sẽ dùng (MP4).
    pub encoder: Option<String>,
    pub warnings: Vec<ExportWarning>,
}

/// Dung lượng ước tính (byte) của file xuất.
pub fn estimate_bytes(s: &ExportSettings, w: u32, h: u32, frames: u32, has_audio: bool) -> u64 {
    let secs = frames as f64 / s.fps.max(1) as f64;
    match s.format {
        ExportFormat::Mp4 => {
            let v = video_bitrate(w, h, s.fps, s.quality, s.codec) as f64;
            let a = if has_audio { AUDIO_BITRATE as f64 } else { 0.0 };
            ((v + a) * secs / 8.0) as u64
        }
        // gifski trên nội dung màn hình: ~0,12 byte/điểm ảnh/frame ở chất lượng cao.
        ExportFormat::Gif => {
            let bpp = match s.quality {
                ExportQuality::Standard => 0.08,
                ExportQuality::High => 0.12,
                ExportQuality::Max => 0.18,
            };
            (w as f64 * h as f64 * frames as f64 * bpp) as u64
        }
    }
}

/// Đường dẫn xuất sau khi sửa tên file không hợp lệ và đặt đúng đuôi.
pub fn normalized_output(s: &ExportSettings) -> (PathBuf, Option<ExportWarning>) {
    let out = with_format_extension(&s.output, s.format);
    let stem = out.file_stem().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
    let clean = mochi_recorder::finalize::sanitize_file_name(&stem);
    if clean == stem {
        return (out, None);
    }
    let fixed = out.with_file_name(format!("{clean}.{}", out.extension().unwrap().to_string_lossy()));
    (fixed, Some(ExportWarning::FileNameChanged { from: stem, to: clean }))
}

fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".mochi-write-test-{}", uuid::Uuid::new_v4().simple()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

/// Media mà đoạn xuất cần nhưng không có trên đĩa: (id, tên).
pub fn missing_media(project: &Project, paths: &HashMap<String, PathBuf>, range: &mochi_project::TimeRange) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for c in &project.clips {
        if c.end() <= range.start || c.start >= range.end || matches!(c.content, ClipContent::Text { .. } | ClipContent::Blur { .. }) {
            continue;
        }
        let Some(id) = &c.media_id else { continue };
        if paths.contains_key(id) || out.iter().any(|(i, _)| i == id) {
            continue;
        }
        let name = project.media.iter().find(|m| &m.id == id).map(|m| m.name.clone()).unwrap_or_default();
        out.push((id.clone(), name));
    }
    out
}

/// Toàn bộ kiểm tra trước khi xuất (K-X1, K-X2, K-X3, quyền ghi, dung lượng, đoạn xuất).
/// `paths`: media id → đường dẫn (chỉ media đang có trên đĩa).
pub fn preflight(
    project: &Project,
    paths: &HashMap<String, PathBuf>,
    s: &ExportSettings,
) -> Result<ExportEstimate, ExportError> {
    if project.clips.is_empty() {
        return Err(ExportError::EmptyProject);
    }
    let range = export_range(project, s);
    let total = duration_of(project);
    if range.end <= range.start || range.start.0 < 0 || range.start >= total || s.fps == 0 || s.short_side < 16 {
        return Err(ExportError::InvalidRange);
    }
    let mut warnings = Vec::new();

    let (output, renamed) = normalized_output(s);
    warnings.extend(renamed);

    // K-X1
    if output.exists() && !s.overwrite {
        return Err(ExportError::FileExists { path: output });
    }

    // K-X2
    let missing = missing_media(project, paths, &range);
    if !missing.is_empty() {
        let names: Vec<String> = missing.iter().map(|(_, n)| n.clone()).collect();
        if !s.allow_black_fill {
            return Err(ExportError::MissingMedia { ids: missing.into_iter().map(|(i, _)| i).collect(), names });
        }
        warnings.push(ExportWarning::BlackFill { names });
    }

    // K-X3
    let mut encoder = None;
    if s.format == ExportFormat::Mp4 {
        let support = caps::support_for(s.codec);
        match support.and_then(|c| c.encoder.clone()) {
            Some(e) => encoder = Some(e),
            None => {
                return Err(ExportError::UnsupportedEncoder {
                    codec: s.codec,
                    reason: support.and_then(|c| c.reason.clone()).unwrap_or_default(),
                });
            }
        }
    }

    let dir = output.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    if !dir.is_dir() || !writable(&dir) {
        return Err(ExportError::NotWritable { dir });
    }

    let (width, height) = output_size(project, s.short_side);
    let frames = frame_count(&range, s.fps);
    let has_audio = project.clips.iter().any(|c| !c.muted && matches!(c.content, ClipContent::Recording { .. } | ClipContent::Video | ClipContent::Audio));
    let bytes = estimate_bytes(s, width, height, frames, has_audio);
    let free = mochi_recorder::disk::free_bytes(&dir).unwrap_or(u64::MAX);
    let needed = bytes + bytes / 10 + 50 * 1024 * 1024;
    if free < needed {
        return Err(ExportError::DiskFull { needed_bytes: needed, free_bytes: free });
    }
    if s.format == ExportFormat::Gif && bytes > GIF_WARN_BYTES {
        warnings.push(ExportWarning::GifTooLarge { bytes });
    }

    Ok(ExportEstimate {
        width,
        height,
        frames,
        duration_secs: (range.end.0 - range.start.0) as f64 / mochi_project::TICKS_PER_SECOND as f64,
        bytes,
        output,
        encoder,
        warnings,
    })
}
