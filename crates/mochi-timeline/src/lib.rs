//! mochi-timeline — mọi chỉnh sửa timeline đi qua `apply(project, op)` (E02–E04).
//!
//! Mỗi thao tác là hàm thuần trên bản sao project; sau đó `validate` kiểm luật trên TOÀN BỘ
//! project. Thao tác vi phạm luật thì bị từ chối và project giữ nguyên (không có trạng thái dở).
//! - K-E2: zoom / hiệu ứng con trỏ chỉ cho clip bản quay.
//! - K-E5: hai clip không chồng nhau trên cùng một track.
//! - Track khóa thì không sửa được clip trên đó.
//! - Giá trị ngoài giới hạn (tốc độ, âm lượng…) được kẹp về giới hạn gần nhất.

mod history;
mod ops;

pub use history::History;
pub use ops::{EditOp, Edge};

use mochi_project::{Clip, ClipContent, MediaKind, Project, Ticks, TrackKind};
use serde::Serialize;

pub const MIN_SPEED: f64 = 0.25;
pub const MAX_SPEED: f64 = 4.0;
pub const MIN_VOLUME_DB: f64 = -60.0;
pub const MAX_VOLUME_DB: f64 = 12.0;
/// Clip ngắn nhất: 1/60 giây.
pub const MIN_CLIP: Ticks = Ticks(mochi_project::TICKS_PER_SECOND / 60);

#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum TimelineError {
    #[error("không thấy {what}")]
    NotFound { what: String },
    /// K-E5
    #[error("clip chồng lên clip khác trên cùng track")]
    #[serde(rename_all = "camelCase")]
    Overlap { clip_id: String, other_id: String },
    #[error("track đang khóa")]
    #[serde(rename_all = "camelCase")]
    TrackLocked { track_id: String },
    /// Ví dụ: kéo file âm thanh vào track hình.
    #[error("media không hợp với loại track")]
    WrongTrackKind,
    /// K-E2
    #[error("hiệu ứng chỉ áp được cho clip bản quay")]
    EffectNotSupported,
    #[error("thao tác không hợp lệ: {detail}")]
    Invalid { detail: String },
}

fn not_found(what: impl Into<String>) -> TimelineError {
    TimelineError::NotFound { what: what.into() }
}

/// Áp một thao tác lên project. Thành công → project mới; thất bại → lỗi, project cũ không đổi.
pub fn apply(project: &Project, op: &EditOp) -> Result<Project, TimelineError> {
    let mut next = project.clone();
    ops::apply_in_place(&mut next, op)?;
    ops::pack_magnetic(&mut next);
    validate(&next)?;
    Ok(next)
}

/// Kiểm luật trên toàn project (chạy sau MỌI thao tác).
pub fn validate(p: &Project) -> Result<(), TimelineError> {
    for c in &p.clips {
        p.tracks
            .iter()
            .find(|t| t.id == c.track_id)
            .ok_or_else(|| not_found(format!("track {}", c.track_id)))?;
        if c.source_out <= c.source_in {
            return Err(TimelineError::Invalid {
                detail: format!("clip {} rỗng", c.id),
            });
        }
        if let Some(mid) = &c.media_id {
            p.media
                .iter()
                .find(|m| &m.id == mid)
                .ok_or_else(|| not_found(format!("media {mid}")))?;
        }
    }
    // K-E5: sắp theo start trong từng track rồi so cặp liền kề.
    for t in &p.tracks {
        let mut clips: Vec<&Clip> = p.clips.iter().filter(|c| c.track_id == t.id).collect();
        clips.sort_by_key(|c| c.start);
        for w in clips.windows(2) {
            if w[0].end() > w[1].start {
                return Err(TimelineError::Overlap {
                    clip_id: w[1].id.clone(),
                    other_id: w[0].id.clone(),
                });
            }
        }
        if clips.iter().any(|c| c.start < Ticks::ZERO) {
            return Err(TimelineError::Invalid {
                detail: "clip bắt đầu trước 0".into(),
            });
        }
    }
    Ok(())
}

/// Track nào nhận được loại media nào.
pub fn track_accepts(kind: TrackKind, content: &ClipContent) -> bool {
    match kind {
        TrackKind::Video => matches!(
            content,
            ClipContent::Recording { .. } | ClipContent::Video | ClipContent::Image
        ),
        TrackKind::Audio => matches!(content, ClipContent::Audio),
        TrackKind::Overlay => content.is_overlay(),
    }
}

pub fn content_for(kind: MediaKind) -> ClipContent {
    match kind {
        MediaKind::Recording => ClipContent::Recording { fx: Default::default() },
        MediaKind::Video => ClipContent::Video,
        MediaKind::Audio => ClipContent::Audio,
        MediaKind::Image => ClipContent::Image,
    }
}

/// Độ dài timeline (điểm kết thúc của clip cuối cùng).
pub fn timeline_duration(p: &Project) -> Ticks {
    p.clips.iter().map(Clip::end).max().unwrap_or(Ticks::ZERO)
}
