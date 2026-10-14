//! S04: sinh vùng zoom tự động từ dữ liệu con trỏ / bàn phím của bản quay (plan mục 8).
//!
//! 1. Sự kiện: mỗi lần nhấn chuột, mỗi "đợt gõ" (≥ 3 phím trong 1 giây).
//! 2. Gom cụm: cách nhau < 2,5 s VÀ < 15% đường chéo màn hình (vị trí lấy từ con trỏ lúc đó).
//! 3. Mỗi cụm → vùng zoom [đầu − 0,4 s, cuối + 1,2 s], tối thiểu 1,5 s, 2×, bám con trỏ.
//! 4. Gộp vùng cách nhau < 1 s; bỏ cụm nằm trọn trong 0,5 s đầu.
//! Thời gian là thời gian TRONG MEDIA (các segment nối liền nhau).

use crate::{Ticks, ZoomSegment, ZoomTarget};
use std::path::Path;

const CLUSTER_GAP: f64 = 2.5;
const CLUSTER_DIST: f64 = 0.15;
const LEAD: f64 = 0.4;
const TAIL: f64 = 1.2;
const MIN_LEN: f64 = 1.5;
const MERGE_GAP: f64 = 1.0;
const SKIP_HEAD: f64 = 0.5;
pub const DEFAULT_AMOUNT: f64 = 2.0;

#[derive(Debug, Clone, Copy)]
struct Event {
    t: f64,
    x: f64,
    y: f64,
}

/// Đọc sự kiện của mọi segment trong `recording_dir`, trả về các vùng zoom `auto = true`.
pub fn generate(recording_dir: &Path) -> Result<Vec<ZoomSegment>, String> {
    let meta = cap_project::RecordingMeta::load_for_project(recording_dir).map_err(|e| e.to_string())?;
    let studio = meta.studio_meta().ok_or("không phải bản quay studio")?;
    let mut events = Vec::new();
    let mut offset = 0.0;
    let mut total = 0.0;
    let segments: Vec<(cap_project::CursorEvents, cap_project::KeyboardEvents, f64)> = match studio {
        cap_project::StudioRecordingMeta::SingleSegment { segment } => {
            let cursor = segment
                .cursor
                .as_ref()
                .and_then(|p| cap_project::CursorEvents::load_from_file(&meta.path(p)).ok())
                .unwrap_or_default();
            let len = crate::media::probe(&meta.path(&segment.display.path)).map(|m| m.duration.secs()).unwrap_or(0.0);
            vec![(cursor, Default::default(), len)]
        }
        cap_project::StudioRecordingMeta::MultipleSegments { inner } => inner
            .segments
            .iter()
            .map(|s| {
                let len = crate::media::probe(&meta.path(&s.display.path)).map(|m| m.duration.secs()).unwrap_or(0.0);
                (s.cursor_events(&meta), s.keyboard_events(&meta), len)
            })
            .collect(),
    };
    for (cursor, keyboard, len) in &segments {
        let pos_at = |t_ms: f64| -> (f64, f64) {
            // Vị trí con trỏ gần nhất trước (hoặc tại) thời điểm t.
            let i = cursor.moves.partition_point(|m| m.time_ms <= t_ms);
            cursor
                .moves
                .get(i.saturating_sub(1))
                .or(cursor.moves.first())
                .map(|m| (m.x, m.y))
                .unwrap_or((0.5, 0.5))
        };
        for c in cursor.clicks.iter().filter(|c| c.down) {
            let (x, y) = pos_at(c.time_ms);
            events.push(Event { t: offset + c.time_ms / 1000.0, x, y });
        }
        // Đợt gõ: ≥ 3 phím nhấn trong 1 giây.
        let downs: Vec<f64> = keyboard.presses.iter().filter(|k| k.down).map(|k| k.time_ms).collect();
        let mut i = 0;
        while i + 2 < downs.len() {
            if downs[i + 2] - downs[i] <= 1000.0 {
                let (x, y) = pos_at(downs[i]);
                events.push(Event { t: offset + downs[i] / 1000.0, x, y });
                i += 3;
            } else {
                i += 1;
            }
        }
        offset += len;
        total += len;
    }
    Ok(from_events(events, total))
}

fn from_events(mut events: Vec<Event>, total: f64) -> Vec<ZoomSegment> {
    events.sort_by(|a, b| a.t.total_cmp(&b.t));
    // Gom cụm.
    let mut clusters: Vec<Vec<Event>> = Vec::new();
    for e in events {
        let joins = clusters.last().and_then(|c| c.last()).is_some_and(|last| {
            e.t - last.t < CLUSTER_GAP && ((e.x - last.x).powi(2) + (e.y - last.y).powi(2)).sqrt() < CLUSTER_DIST * 2f64.sqrt()
        });
        if joins {
            clusters.last_mut().unwrap().push(e);
        } else {
            clusters.push(vec![e]);
        }
    }
    // Cụm → vùng.
    let mut ranges: Vec<(f64, f64)> = clusters
        .iter()
        .filter(|c| c.last().unwrap().t >= SKIP_HEAD)
        .map(|c| {
            let start = (c[0].t - LEAD).max(0.0);
            let mut end = c.last().unwrap().t + TAIL;
            if end - start < MIN_LEN {
                end = start + MIN_LEN;
            }
            (start, end.min(total.max(start + 0.1)))
        })
        .collect();
    // Gộp vùng gần nhau.
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 - last.1 < MERGE_GAP => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    merged
        .into_iter()
        .map(|(s, e)| ZoomSegment {
            id: uuid::Uuid::new_v4().to_string(),
            start: Ticks::from_secs(s),
            end: Ticks::from_secs(e),
            amount: DEFAULT_AMOUNT,
            target: ZoomTarget::Follow,
            auto: true,
        })
        .collect()
}
