use crate::{
    MAX_SPEED, MAX_VOLUME_DB, MIN_CLIP, MIN_SPEED, MIN_VOLUME_DB, TimelineError, content_for,
    not_found, track_accepts,
};
use mochi_project::{
    Clip, ClipContent, MediaKind, Project, RecordingFx, Ticks, Track, TrackKind, Transform, ZoomSegment,
    ZoomTarget,
};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Start,
    End,
}

/// Mọi chỉnh sửa timeline. UI gửi đúng một `EditOp` qua lệnh `timeline_apply`.
#[derive(Debug, Clone, Deserialize, serde::Serialize, specta::Type)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum EditOp {
    #[serde(rename_all = "camelCase")]
    AddTrack { kind: TrackKind },
    /// Chỉ xóa được track rỗng.
    #[serde(rename_all = "camelCase")]
    RemoveTrack { track_id: String },
    #[serde(rename_all = "camelCase")]
    SetTrack {
        track_id: String,
        name: Option<String>,
        locked: Option<bool>,
        muted: Option<bool>,
        hidden: Option<bool>,
        /// E11: âm lượng cả track (dB).
        #[serde(default)]
        volume_db: Option<f64>,
    },
    /// Thả media lên timeline. `track_id` None → track phù hợp đầu tiên còn chỗ (tạo mới nếu cần).
    #[serde(rename_all = "camelCase")]
    AddClip {
        media_id: String,
        track_id: Option<String>,
        start: Ticks,
    },
    /// Dời các clip đi `delta`; có `track_id` thì chuyển sang track đó.
    #[serde(rename_all = "camelCase")]
    MoveClips {
        clip_ids: Vec<String>,
        delta: Ticks,
        track_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    TrimClip { clip_id: String, edge: Edge, to: Ticks },
    #[serde(rename_all = "camelCase")]
    SplitClip { clip_id: String, at: Ticks },
    /// `ripple` = dồn các clip phía sau trên cùng track vào chỗ trống.
    #[serde(rename_all = "camelCase")]
    DeleteClips { clip_ids: Vec<String>, ripple: bool },
    #[serde(rename_all = "camelCase")]
    SetClipSpeed { clip_id: String, speed: f64 },
    #[serde(rename_all = "camelCase")]
    SetClipAudio {
        clip_id: String,
        volume_db: Option<f64>,
        fade_in: Option<Ticks>,
        fade_out: Option<Ticks>,
        muted: Option<bool>,
    },
    #[serde(rename_all = "camelCase")]
    SetClipTransform { clip_id: String, transform: Transform },
    /// E05–E08: thay toàn bộ hiệu ứng của một clip bản quay (K-E2: chỉ clip bản quay).
    #[serde(rename_all = "camelCase")]
    SetRecordingFx { clip_id: String, fx: RecordingFx },
    /// E05: thêm vùng zoom (thời gian TRONG MEDIA của clip).
    #[serde(rename_all = "camelCase")]
    AddZoom { clip_id: String, start: Ticks, end: Ticks, amount: f64 },
    /// E05: sửa vùng zoom (người dùng sửa → không còn là vùng tự động).
    #[serde(rename_all = "camelCase")]
    UpdateZoom {
        clip_id: String,
        zoom_id: String,
        start: Option<Ticks>,
        end: Option<Ticks>,
        amount: Option<f64>,
        target: Option<ZoomTarget>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveZoom { clip_id: String, zoom_id: String },
    /// E09: kích thước / tỉ lệ khung hình đầu ra.
    #[serde(rename_all = "camelCase")]
    SetCanvas { width: u32, height: u32 },
    /// E10: thêm chữ hoặc vùng làm mờ trên track overlay (tạo track nếu chưa có).
    #[serde(rename_all = "camelCase")]
    AddOverlay {
        content: ClipContent,
        start: Ticks,
        duration: Ticks,
        track_id: Option<String>,
    },
    /// E10: sửa nội dung chữ / vùng làm mờ.
    #[serde(rename_all = "camelCase")]
    SetOverlay { clip_id: String, content: ClipContent },
    /// E11: tách tiếng của clip hình ra một clip tiếng riêng (clip gốc bị tắt tiếng).
    #[serde(rename_all = "camelCase")]
    DetachAudio { clip_id: String },
}

pub(crate) fn apply_in_place(p: &mut Project, op: &EditOp) -> Result<(), TimelineError> {
    match op {
        EditOp::AddTrack { kind } => {
            let n = p.tracks.iter().filter(|t| t.kind == *kind).count() + 1;
            let name = match kind {
                TrackKind::Video => format!("Video {n}"),
                TrackKind::Audio => format!("Audio {n}"),
                TrackKind::Overlay => format!("Overlay {n}"),
            };
            p.tracks.push(new_track(*kind, name));
        }
        EditOp::RemoveTrack { track_id } => {
            track(p, track_id)?;
            if p.clips.iter().any(|c| &c.track_id == track_id) {
                return Err(TimelineError::Invalid {
                    detail: "track còn clip".into(),
                });
            }
            p.tracks.retain(|t| &t.id != track_id);
        }
        EditOp::SetTrack {
            track_id,
            name,
            locked,
            muted,
            hidden,
            volume_db,
        } => {
            let t = p
                .tracks
                .iter_mut()
                .find(|t| &t.id == track_id)
                .ok_or_else(|| not_found("track"))?;
            if let Some(v) = name {
                let v = v.trim();
                if !v.is_empty() {
                    t.name = v.chars().take(60).collect();
                }
            }
            if let Some(v) = locked {
                t.locked = *v;
            }
            if let Some(v) = muted {
                t.muted = *v;
            }
            if let Some(v) = hidden {
                t.hidden = *v;
            }
            if let Some(v) = volume_db {
                t.volume_db = v.clamp(MIN_VOLUME_DB, MAX_VOLUME_DB);
            }
        }
        EditOp::AddClip {
            media_id,
            track_id,
            start,
        } => add_clip(p, media_id, track_id.as_deref(), (*start).max(Ticks::ZERO))?,
        EditOp::MoveClips {
            clip_ids,
            delta,
            track_id,
        } => {
            if let Some(tid) = track_id {
                ensure_unlocked(p, tid)?;
            }
            for id in clip_ids {
                let content = clip(p, id)?.content.clone();
                ensure_unlocked(p, &clip(p, id)?.track_id.clone())?;
                if let Some(tid) = track_id
                    && !track_accepts(track(p, tid)?.kind, &content)
                {
                    return Err(TimelineError::WrongTrackKind);
                }
                let c = clip_mut(p, id)?;
                c.start = c.start + *delta;
                if let Some(tid) = track_id {
                    c.track_id = tid.clone();
                }
            }
        }
        EditOp::TrimClip { clip_id, edge, to } => trim(p, clip_id, *edge, *to)?,
        EditOp::SplitClip { clip_id, at } => split(p, clip_id, *at)?,
        EditOp::DeleteClips { clip_ids, ripple } => delete(p, clip_ids, *ripple)?,
        EditOp::SetClipSpeed { clip_id, speed } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = clip_mut(p, clip_id)?;
            let speed = if speed.is_finite() { *speed } else { 1.0 };
            c.speed = speed.clamp(MIN_SPEED, MAX_SPEED);
        }
        EditOp::SetClipAudio {
            clip_id,
            volume_db,
            fade_in,
            fade_out,
            muted,
        } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = clip_mut(p, clip_id)?;
            let dur = c.duration();
            if let Some(v) = volume_db {
                c.volume_db = v.clamp(MIN_VOLUME_DB, MAX_VOLUME_DB);
            }
            if let Some(v) = fade_in {
                c.fade_in = Ticks(v.0.clamp(0, dur.0));
            }
            if let Some(v) = fade_out {
                c.fade_out = Ticks(v.0.clamp(0, dur.0));
            }
            if let Some(v) = muted {
                c.muted = *v;
            }
        }
        EditOp::SetClipTransform { clip_id, transform } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = clip_mut(p, clip_id)?;
            c.transform = Some(Transform {
                x: transform.x.clamp(-1.0, 2.0),
                y: transform.y.clamp(-1.0, 2.0),
                scale: transform.scale.clamp(0.05, 10.0),
                opacity: transform.opacity.clamp(0.0, 1.0),
            });
        }
        EditOp::SetRecordingFx { clip_id, fx } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = recording_clip_mut(p, clip_id)?;
            // Kẹp trong project (không chỉ lúc render) — giá trị ngoài giới hạn không vào được file.
            let mut fx = fx.clone().clamped();
            sanitize_zooms(&mut fx.zoom.segments);
            if let ClipContent::Recording { fx: current } = &mut c.content {
                *current = fx;
            }
        }
        EditOp::AddZoom { clip_id, start, end, amount } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = recording_clip_mut(p, clip_id)?;
            if let ClipContent::Recording { fx } = &mut c.content {
                fx.zoom.segments.push(ZoomSegment {
                    id: uuid::Uuid::new_v4().to_string(),
                    start: *start,
                    end: *end,
                    amount: *amount,
                    target: ZoomTarget::Follow,
                    auto: false,
                });
                sanitize_zooms(&mut fx.zoom.segments);
            }
        }
        EditOp::UpdateZoom { clip_id, zoom_id, start, end, amount, target } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = recording_clip_mut(p, clip_id)?;
            if let ClipContent::Recording { fx } = &mut c.content {
                let z = fx
                    .zoom
                    .segments
                    .iter_mut()
                    .find(|z| &z.id == zoom_id)
                    .ok_or_else(|| not_found("zoom"))?;
                if let Some(v) = start {
                    z.start = *v;
                }
                if let Some(v) = end {
                    z.end = *v;
                }
                if let Some(v) = amount {
                    z.amount = *v;
                }
                if let Some(v) = target {
                    z.target = *v;
                }
                z.auto = false;
                sanitize_zooms(&mut fx.zoom.segments);
            }
        }
        EditOp::RemoveZoom { clip_id, zoom_id } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = recording_clip_mut(p, clip_id)?;
            if let ClipContent::Recording { fx } = &mut c.content {
                fx.zoom.segments.retain(|z| &z.id != zoom_id);
            }
        }
        EditOp::SetCanvas { width, height } => {
            // Chẵn để encoder chấp nhận; giới hạn 240p–8K.
            p.canvas.width = (*width).clamp(240, 7680) & !1;
            p.canvas.height = (*height).clamp(240, 7680) & !1;
        }
        EditOp::AddOverlay { content, start, duration, track_id } => {
            if !content.is_overlay() {
                return Err(TimelineError::WrongTrackKind);
            }
            let start = (*start).max(Ticks::ZERO);
            let len = Ticks((*duration).0.max(MIN_CLIP.0));
            let tid = match track_id {
                Some(t) => {
                    ensure_unlocked(p, t)?;
                    if track(p, t)?.kind != TrackKind::Overlay {
                        return Err(TimelineError::WrongTrackKind);
                    }
                    t.clone()
                }
                None => free_track(p, TrackKind::Overlay, start, start + len),
            };
            let is_text = matches!(content, ClipContent::Text { .. });
            p.clips.push(Clip {
                id: uuid::Uuid::new_v4().to_string(),
                track_id: tid,
                media_id: None,
                start,
                source_in: Ticks::ZERO,
                source_out: len,
                speed: 1.0,
                volume_db: 0.0,
                fade_in: Ticks::ZERO,
                fade_out: Ticks::ZERO,
                muted: false,
                transform: is_text.then_some(Transform { x: 0.5, y: 0.85, scale: 1.0, opacity: 1.0 }),
                content: sanitize_overlay(content.clone()),
            });
        }
        EditOp::SetOverlay { clip_id, content } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = clip_mut(p, clip_id)?;
            if !c.content.is_overlay() || !content.is_overlay() {
                return Err(TimelineError::EffectNotSupported);
            }
            c.content = sanitize_overlay(content.clone());
        }
        EditOp::DetachAudio { clip_id } => {
            ensure_clip_unlocked(p, clip_id)?;
            let original = clip(p, clip_id)?.clone();
            let media = original
                .media_id
                .as_ref()
                .and_then(|m| p.media.iter().find(|x| &x.id == m))
                .ok_or_else(|| not_found("media"))?;
            let has_audio = media.audio.is_some() || media.kind == MediaKind::Recording;
            let is_picture = matches!(original.content, ClipContent::Video | ClipContent::Recording { .. });
            if !is_picture || !has_audio {
                return Err(TimelineError::Invalid { detail: "clip không có tiếng để tách".into() });
            }
            let tid = free_track(p, TrackKind::Audio, original.start, original.end());
            let mut audio = original.clone();
            audio.id = uuid::Uuid::new_v4().to_string();
            audio.track_id = tid;
            audio.content = ClipContent::Audio;
            audio.transform = None;
            p.clips.push(audio);
            clip_mut(p, clip_id)?.muted = true;
        }
    }
    p.updated_at = now();
    Ok(())
}

/// K-E2: hiệu ứng bản quay chỉ áp cho clip bản quay.
fn recording_clip_mut<'a>(p: &'a mut Project, id: &str) -> Result<&'a mut Clip, TimelineError> {
    let c = clip_mut(p, id)?;
    if matches!(c.content, ClipContent::Recording { .. }) {
        Ok(c)
    } else {
        Err(TimelineError::EffectNotSupported)
    }
}

/// Mức zoom 1–5, điểm cố định 0–1, bỏ vùng rỗng; không cho chồng nhau (vùng sau bị cắt đầu).
/// Vùng nằm ngoài đoạn clip đang dùng vẫn giữ: kéo dài clip lại thì zoom quay lại.
fn sanitize_zooms(zs: &mut Vec<ZoomSegment>) {
    for z in zs.iter_mut() {
        z.amount = if z.amount.is_finite() { z.amount.clamp(1.0, 5.0) } else { 2.0 };
        if let ZoomTarget::Fixed { x, y } = &mut z.target {
            *x = x.clamp(0.0, 1.0);
            *y = y.clamp(0.0, 1.0);
        }
        if z.end < z.start {
            std::mem::swap(&mut z.start, &mut z.end);
        }
    }
    zs.sort_by_key(|z| z.start);
    let mut prev_end = Ticks(i64::MIN);
    for z in zs.iter_mut() {
        z.start = z.start.max(prev_end);
        if z.end > z.start {
            prev_end = z.end;
        }
    }
    zs.retain(|z| z.end.0 - z.start.0 >= MIN_CLIP.0);
}

fn sanitize_overlay(content: ClipContent) -> ClipContent {
    match content {
        ClipContent::Text { text, font, size_px, color, background } => ClipContent::Text {
            text: text.chars().take(500).collect(),
            font: if font.trim().is_empty() { "Segoe UI".into() } else { font },
            size_px: if size_px.is_finite() { size_px.clamp(8.0, 400.0) } else { 64.0 },
            color,
            background,
        },
        ClipContent::Blur { x, y, width, height, pixelate, strength } => {
            let x = x.clamp(0.0, 1.0);
            let y = y.clamp(0.0, 1.0);
            ClipContent::Blur {
                x,
                y,
                width: width.clamp(0.01, (1.0 - x).max(0.01)),
                height: height.clamp(0.01, (1.0 - y).max(0.01)),
                pixelate,
                strength: strength.clamp(0.0, 1.0),
            }
        }
        other => other,
    }
}

/// Track loại `kind` còn trống ở [start, end) (không khóa); không có thì tạo mới (ở trên cùng).
fn free_track(p: &mut Project, kind: TrackKind, start: Ticks, end: Ticks) -> String {
    let found = p
        .tracks
        .iter()
        .filter(|t| t.kind == kind && !t.locked)
        .find(|t| !p.clips.iter().any(|c| c.track_id == t.id && c.start < end && start < c.end()))
        .map(|t| t.id.clone());
    found.unwrap_or_else(|| {
        let n = p.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        let name = match kind {
            TrackKind::Video => format!("Video {n}"),
            TrackKind::Audio => format!("Audio {n}"),
            TrackKind::Overlay => format!("Overlay {n}"),
        };
        let t = new_track(kind, name);
        let id = t.id.clone();
        p.tracks.push(t);
        id
    })
}

fn add_clip(p: &mut Project, media_id: &str, track_id: Option<&str>, start: Ticks) -> Result<(), TimelineError> {
    let media = p
        .media
        .iter()
        .find(|m| m.id == media_id)
        .ok_or_else(|| not_found("media"))?
        .clone();
    let content = content_for(media.kind);
    let duration = media.duration;
    let make = |track_id: String| Clip {
        id: uuid::Uuid::new_v4().to_string(),
        track_id,
        media_id: Some(media.id.clone()),
        start,
        source_in: Ticks::ZERO,
        source_out: duration,
        speed: 1.0,
        volume_db: 0.0,
        fade_in: Ticks::ZERO,
        fade_out: Ticks::ZERO,
        muted: false,
        transform: None,
        content: content.clone(),
    };

    if let Some(tid) = track_id {
        ensure_unlocked(p, tid)?;
        if !track_accepts(track(p, tid)?.kind, &content) {
            return Err(TimelineError::WrongTrackKind);
        }
        p.clips.push(make(tid.to_string()));
        return Ok(());
    }

    // Tự chọn track: track phù hợp, không khóa, còn trống ở [start, start + duration).
    let probe = make(String::new());
    let free = p
        .tracks
        .iter()
        .filter(|t| !t.locked && track_accepts(t.kind, &content))
        .find(|t| {
            !p.clips
                .iter()
                .any(|c| c.track_id == t.id && c.start < probe.end() && probe.start < c.end())
        })
        .map(|t| t.id.clone());
    let tid = match free {
        Some(id) => id,
        None => {
            let kind = if media.kind == MediaKind::Audio {
                TrackKind::Audio
            } else {
                TrackKind::Video
            };
            let n = p.tracks.iter().filter(|t| t.kind == kind).count() + 1;
            let t = new_track(kind, format!("{} {n}", if kind == TrackKind::Audio { "Audio" } else { "Video" }));
            let id = t.id.clone();
            p.tracks.push(t);
            id
        }
    };
    p.clips.push(make(tid));
    Ok(())
}

fn trim(p: &mut Project, clip_id: &str, edge: Edge, to: Ticks) -> Result<(), TimelineError> {
    ensure_clip_unlocked(p, clip_id)?;
    let c = clip(p, clip_id)?.clone();
    // Ảnh tĩnh kéo dài tùy ý; hình/tiếng bị giới hạn bởi độ dài media.
    let media_len = match &c.media_id {
        Some(mid) => {
            let m = p.media.iter().find(|m| &m.id == mid).ok_or_else(|| not_found("media"))?;
            (m.kind != MediaKind::Image).then_some(m.duration)
        }
        None => None,
    };
    let speed = c.speed;
    let to_src = |dt: i64| Ticks((dt as f64 * speed).round() as i64);
    // Kẹp vào mép clip kề bên trên cùng track (không bao giờ tạo chồng lấn khi trim).
    let prev_end = p
        .clips
        .iter()
        .filter(|o| o.track_id == c.track_id && o.id != c.id && o.end() <= c.start)
        .map(Clip::end)
        .max()
        .unwrap_or(Ticks::ZERO);
    let next_start = p
        .clips
        .iter()
        .filter(|o| o.track_id == c.track_id && o.id != c.id && o.start >= c.end())
        .map(|o| o.start)
        .min();
    let c = clip_mut(p, clip_id)?;
    match edge {
        Edge::Start => {
            // Không kéo trước đầu media, không quá điểm cuối - MIN_CLIP.
            let earliest = if media_len.is_some() {
                c.start - Ticks((c.source_in.0 as f64 / speed).round() as i64)
            } else {
                Ticks(i64::MIN / 4)
            };
            let t = to.max(earliest).max(prev_end).min(c.end() - MIN_CLIP);
            let shift = to_src(t.0 - c.start.0);
            c.source_in = c.source_in + shift;
            c.start = t;
            if media_len.is_none() && c.source_in < Ticks::ZERO {
                // ảnh: dời cả cửa sổ nguồn về 0
                c.source_out = c.source_out - c.source_in;
                c.source_in = Ticks::ZERO;
            }
        }
        Edge::End => {
            let latest = match media_len {
                Some(len) => c.start + Ticks(((len.0 - c.source_in.0) as f64 / speed).round() as i64),
                None => Ticks(i64::MAX / 4),
            };
            let latest = next_start.map_or(latest, |n| latest.min(n));
            let t = to.min(latest).max(c.start + MIN_CLIP);
            c.source_out = c.source_in + to_src(t.0 - c.start.0);
        }
    }
    let dur = c.duration();
    c.fade_in = Ticks(c.fade_in.0.min(dur.0));
    c.fade_out = Ticks(c.fade_out.0.min(dur.0));
    Ok(())
}

fn split(p: &mut Project, clip_id: &str, at: Ticks) -> Result<(), TimelineError> {
    ensure_clip_unlocked(p, clip_id)?;
    let c = clip(p, clip_id)?.clone();
    if at < c.start + MIN_CLIP || at > c.end() - MIN_CLIP {
        return Err(TimelineError::Invalid {
            detail: "điểm cắt nằm ngoài clip".into(),
        });
    }
    let cut_src = c.source_time_at(at);
    let mut right = c.clone();
    right.id = uuid::Uuid::new_v4().to_string();
    right.start = at;
    right.source_in = cut_src;
    right.fade_in = Ticks::ZERO;
    // Hiệu ứng theo thời gian trong media nên hai nửa giữ nguyên fx.
    let left = clip_mut(p, clip_id)?;
    left.source_out = cut_src;
    left.fade_out = Ticks::ZERO;
    let idx = p.clips.iter().position(|x| x.id == clip_id).unwrap();
    p.clips.insert(idx + 1, right);
    Ok(())
}

fn delete(p: &mut Project, ids: &[String], ripple: bool) -> Result<(), TimelineError> {
    let mut removed: Vec<Clip> = Vec::new();
    for id in ids {
        ensure_clip_unlocked(p, id)?;
        removed.push(clip(p, id)?.clone());
    }
    p.clips.retain(|c| !ids.contains(&c.id));
    if ripple {
        // Xử lý từ clip muộn nhất về sớm nhất để các lần dồn không chồng sai.
        removed.sort_by_key(|c| std::cmp::Reverse(c.start));
        for r in &removed {
            let gap = r.duration();
            for c in p.clips.iter_mut() {
                if c.track_id == r.track_id && c.start >= r.end() {
                    c.start = c.start - gap;
                }
            }
        }
    }
    Ok(())
}

// ---------- tiện ích ----------

fn new_track(kind: TrackKind, name: String) -> Track {
    Track {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        name,
        locked: false,
        muted: false,
        hidden: false,
        volume_db: 0.0,
    }
}

fn track<'a>(p: &'a Project, id: &str) -> Result<&'a Track, TimelineError> {
    p.tracks.iter().find(|t| t.id == id).ok_or_else(|| not_found("track"))
}

fn clip<'a>(p: &'a Project, id: &str) -> Result<&'a Clip, TimelineError> {
    p.clips.iter().find(|c| c.id == id).ok_or_else(|| not_found("clip"))
}

fn clip_mut<'a>(p: &'a mut Project, id: &str) -> Result<&'a mut Clip, TimelineError> {
    p.clips.iter_mut().find(|c| c.id == id).ok_or_else(|| not_found("clip"))
}

fn ensure_unlocked(p: &Project, track_id: &str) -> Result<(), TimelineError> {
    if track(p, track_id)?.locked {
        Err(TimelineError::TrackLocked {
            track_id: track_id.to_string(),
        })
    } else {
        Ok(())
    }
}

fn ensure_clip_unlocked(p: &Project, clip_id: &str) -> Result<(), TimelineError> {
    let tid = clip(p, clip_id)?.track_id.clone();
    ensure_unlocked(p, &tid)
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}
