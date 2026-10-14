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
    /// Xóa track cùng mọi clip trên đó (UI hỏi xác nhận khi track còn clip). Track khóa → từ chối.
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
    /// E14 (v1): marker.
    #[serde(rename_all = "camelCase")]
    AddMarker { at: Ticks, label: String },
    #[serde(rename_all = "camelCase")]
    UpdateMarker { marker_id: String, at: Option<Ticks>, label: Option<String> },
    #[serde(rename_all = "camelCase")]
    RemoveMarker { marker_id: String },
    /// E04 (v1): chèn khung đứng yên dài `duration` tại `at` (timeline) của clip; `at` = cuối clip →
    /// kéo dài frame cuối. Các clip phía sau trên cùng track (và tiếng đã tách của clip đó) dời theo.
    #[serde(rename_all = "camelCase")]
    FreezeFrame { clip_id: String, at: Ticks, duration: Ticks },
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
    /// Chép nền, con trỏ, webcam của clip này sang mọi clip bản quay khác (giữ zoom riêng của từng clip).
    #[serde(rename_all = "camelCase")]
    ApplyRecordingFxToAll { clip_id: String },
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
            ensure_unlocked(p, track_id)?;
            p.clips.retain(|c| &c.track_id != track_id);
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
        EditOp::FreezeFrame { clip_id, at, duration } => freeze(p, clip_id, *at, *duration)?,
        EditOp::AddMarker { at, label } => {
            p.markers.push(mochi_project::Marker {
                id: uuid::Uuid::new_v4().to_string(),
                at: (*at).max(Ticks::ZERO),
                label: marker_label(label),
            });
            p.markers.sort_by_key(|m| m.at);
        }
        EditOp::UpdateMarker { marker_id, at, label } => {
            let m = p.markers.iter_mut().find(|m| &m.id == marker_id).ok_or_else(|| not_found("marker"))?;
            if let Some(at) = at {
                m.at = (*at).max(Ticks::ZERO);
            }
            if let Some(l) = label {
                m.label = marker_label(l);
            }
            p.markers.sort_by_key(|m| m.at);
        }
        EditOp::RemoveMarker { marker_id } => {
            let before = p.markers.len();
            p.markers.retain(|m| &m.id != marker_id);
            if p.markers.len() == before {
                return Err(not_found("marker"));
            }
        }
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
                rotation: if transform.rotation.is_finite() { transform.rotation.clamp(-3600.0, 3600.0) } else { 0.0 },
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
            sync_recording_siblings(p, clip_id);
        }
        EditOp::ApplyRecordingFxToAll { clip_id } => {
            let ClipContent::Recording { fx: src } = recording_clip_mut(p, clip_id)?.content.clone() else {
                return Err(TimelineError::EffectNotSupported);
            };
            let locked: Vec<String> = p.tracks.iter().filter(|t| t.locked).map(|t| t.id.clone()).collect();
            for c in p.clips.iter_mut().filter(|c| !locked.contains(&c.track_id)) {
                if let ClipContent::Recording { fx } = &mut c.content {
                    fx.background = src.background.clone();
                    fx.cursor = src.cursor.clone();
                    fx.camera = src.camera.clone();
                }
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
            sync_recording_siblings(p, clip_id);
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
            sync_recording_siblings(p, clip_id);
        }
        EditOp::RemoveZoom { clip_id, zoom_id } => {
            ensure_clip_unlocked(p, clip_id)?;
            let c = recording_clip_mut(p, clip_id)?;
            if let ClipContent::Recording { fx } = &mut c.content {
                fx.zoom.segments.retain(|z| &z.id != zoom_id);
            }
            sync_recording_siblings(p, clip_id);
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
                transform: is_text.then_some(Transform { x: 0.5, y: 0.85, scale: 1.0, opacity: 1.0, rotation: 0.0 }),
                content: sanitize_overlay(content.clone()),
                hold: None,
                keyframes: vec![],
                transition_in: None,
                transition_out: None,
                look: Default::default(),
                audio_fx: Default::default(),
                group: None,
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
/// Các clip cắt ra từ cùng một bản quay dùng chung hiệu ứng (zoom lưu theo thời gian trong media nên
/// dùng chung được): sửa một clip → các clip anh em trên track không khóa nhận cùng `fx`. Tránh
/// trường hợp cắt clip rồi chỉnh nền một nửa, nửa kia vẫn mặc định.
fn sync_recording_siblings(p: &mut Project, clip_id: &str) {
    let Some(src) = p.clips.iter().find(|c| c.id == clip_id) else { return };
    let (Some(media), ClipContent::Recording { fx }) = (src.media_id.clone(), src.content.clone()) else { return };
    let locked: Vec<String> = p.tracks.iter().filter(|t| t.locked).map(|t| t.id.clone()).collect();
    for c in p.clips.iter_mut() {
        if c.id != clip_id && c.media_id.as_deref() == Some(media.as_str()) && !locked.contains(&c.track_id)
            && let ClipContent::Recording { fx: other } = &mut c.content
        {
            *other = fx.clone();
        }
    }
}

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
        hold: None,
        keyframes: vec![],
        transition_in: None,
        transition_out: None,
        look: Default::default(),
        audio_fx: Default::default(),
        group: None,
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

/// Tên marker: bỏ khoảng trắng thừa, tối đa 80 ký tự, rỗng → "Marker".
fn marker_label(l: &str) -> String {
    let l: String = l.trim().chars().take(80).collect();
    if l.is_empty() { "Marker".into() } else { l }
}

/// E04: freeze frame / kéo dài frame cuối.
fn freeze(p: &mut Project, clip_id: &str, at: Ticks, duration: Ticks) -> Result<(), TimelineError> {
    ensure_clip_unlocked(p, clip_id)?;
    let c = clip(p, clip_id)?.clone();
    if !matches!(c.content, ClipContent::Recording { .. } | ClipContent::Video) || c.hold.is_some() {
        return Err(TimelineError::Invalid { detail: "chỉ freeze được clip video / bản quay".into() });
    }
    if at < c.start || at > c.end() {
        return Err(TimelineError::Invalid { detail: "đầu phát nằm ngoài clip".into() });
    }
    let duration = Ticks(duration.0.clamp(MIN_CLIP.0, Ticks::from_secs(600.0).0));
    // Khung giữ lại: tại đầu phát, hoặc frame cuối (lùi 1/60 s) nếu ở cuối clip.
    let last = Ticks((c.source_out.0 - MIN_CLIP.0).max(c.source_in.0));
    let held = if at >= c.end() { last } else { Ticks(c.source_time_at(at).0.min(last.0)) };
    if at > c.start && at < c.end() {
        split(p, clip_id, at)?;
    }
    // Dời mọi clip sau `at` trên cùng track + tiếng đã tách của cùng media trên track tiếng.
    let detached: Vec<String> = p
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio && !t.locked)
        .map(|t| t.id.clone())
        .collect();
    for other in p.clips.iter_mut() {
        let same_track = other.track_id == c.track_id;
        let its_audio = c.media_id.is_some() && other.media_id == c.media_id && detached.contains(&other.track_id);
        if (same_track || its_audio) && other.start >= at {
            other.start = other.start + duration;
        }
    }
    let mut still = c.clone();
    still.id = uuid::Uuid::new_v4().to_string();
    still.start = at;
    still.source_in = held;
    still.source_out = held + duration;
    still.speed = 1.0;
    still.hold = Some(held);
    still.keyframes.clear();
    still.transition_in = None;
    still.transition_out = None;
    still.fade_in = Ticks::ZERO;
    still.fade_out = Ticks::ZERO;
    p.clips.push(still);
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
        magnetic: false,
        duck: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use mochi_project::{BackgroundKind, MediaItem};

    fn recording(id: &str) -> MediaItem {
        MediaItem {
            id: id.into(),
            kind: MediaKind::Recording,
            name: id.into(),
            abs_path: "x".into(),
            rel_path: None,
            hash: id.into(),
            size_bytes: 1.0,
            duration: Ticks::from_secs(10.0),
            video: None,
            audio: None,
            recording: None,
        }
    }

    /// Bản quay A cắt làm 2 clip + bản quay B, kèm một track overlay có chữ.
    fn project() -> Project {
        let mut p = mochi_project::new_project("t");
        p.media = vec![recording("a"), recording("b")];
        let v1 = p.tracks[0].id.clone();
        let mut p = crate::apply(&p, &EditOp::AddClip { media_id: "a".into(), track_id: Some(v1.clone()), start: Ticks::ZERO }).unwrap();
        p = crate::apply(&p, &EditOp::AddClip { media_id: "b".into(), track_id: Some(v1), start: Ticks::from_secs(10.0) }).unwrap();
        let a = p.clips[0].id.clone();
        crate::apply(&p, &EditOp::SplitClip { clip_id: a, at: Ticks::from_secs(4.0) }).unwrap()
    }

    fn fx_of(p: &Project, media: &str) -> Vec<RecordingFx> {
        p.clips
            .iter()
            .filter(|c| c.media_id.as_deref() == Some(media))
            .filter_map(|c| match &c.content {
                ClipContent::Recording { fx } => Some(fx.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn editing_one_half_of_a_split_recording_updates_the_other_half() {
        let p = project();
        let first = p.clips.iter().find(|c| c.media_id.as_deref() == Some("a")).unwrap();
        let ClipContent::Recording { fx } = &first.content else { panic!() };
        let mut fx = fx.clone();
        fx.background.padding = 0.0;
        fx.background.rounding = 0.0;
        let p = crate::apply(&p, &EditOp::SetRecordingFx { clip_id: first.id.clone(), fx }).unwrap();
        let a = fx_of(&p, "a");
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|f| f.background.padding == 0.0 && f.background.rounding == 0.0));
        // Bản quay khác không đổi.
        assert_eq!(fx_of(&p, "b")[0].background.padding, RecordingFx::default().background.padding);
    }

    #[test]
    fn apply_to_all_copies_frame_but_keeps_each_clips_zoom() {
        let p = project();
        let b = p.clips.iter().find(|c| c.media_id.as_deref() == Some("b")).unwrap().id.clone();
        let p = crate::apply(&p, &EditOp::AddZoom { clip_id: b.clone(), start: Ticks::ZERO, end: Ticks::from_secs(1.0), amount: 2.0 }).unwrap();
        let mut fx = fx_of(&p, "b")[0].clone();
        fx.background.kind = BackgroundKind::None;
        let p = crate::apply(&p, &EditOp::SetRecordingFx { clip_id: b.clone(), fx }).unwrap();
        let p = crate::apply(&p, &EditOp::ApplyRecordingFxToAll { clip_id: b }).unwrap();
        assert!(fx_of(&p, "a").iter().all(|f| f.background.kind == BackgroundKind::None && f.zoom.segments.is_empty()));
        assert_eq!(fx_of(&p, "b")[0].zoom.segments.len(), 1);
    }

    #[test]
    fn freeze_frame_inserts_a_silent_still_and_shifts_later_clips() {
        let p = project();
        // Clip "a" đã bị cắt ở 4 s: a1 [0,4) a2 [4,10), "b" [10,20).
        let a1 = p.clips.iter().find(|c| c.media_id.as_deref() == Some("a") && c.start == Ticks::ZERO).unwrap().id.clone();
        let p = crate::apply(&p, &EditOp::FreezeFrame { clip_id: a1.clone(), at: Ticks::from_secs(2.0), duration: Ticks::from_secs(3.0) }).unwrap();
        let still = p.clips.iter().find(|c| c.hold.is_some()).unwrap();
        assert_eq!(still.start, Ticks::from_secs(2.0));
        assert_eq!(still.duration(), Ticks::from_secs(3.0));
        assert_eq!(still.hold, Some(Ticks::from_secs(2.0)));
        // Mọi khung trong freeze đều là giây thứ 2 của media.
        assert_eq!(still.source_time_at(Ticks::from_secs(4.5)), Ticks::from_secs(2.0));
        let b = p.clips.iter().find(|c| c.media_id.as_deref() == Some("b")).unwrap();
        assert_eq!(b.start, Ticks::from_secs(13.0));
        // Kéo dài frame cuối của "b".
        let b_id = b.id.clone();
        let end = b.end();
        let p = crate::apply(&p, &EditOp::FreezeFrame { clip_id: b_id, at: end, duration: Ticks::from_secs(1.0) }).unwrap();
        let tail = p.clips.iter().filter(|c| c.hold.is_some()).max_by_key(|c| c.start).unwrap();
        assert_eq!(tail.start, end);
        assert!(tail.hold.unwrap() < Ticks::from_secs(10.0));
    }

    #[test]
    fn removing_a_track_removes_its_clips_unless_locked() {
        let p = project();
        let p = crate::apply(
            &p,
            &EditOp::AddOverlay {
                content: ClipContent::Text { text: "hi".into(), font: "Inter".into(), size_px: 48.0, color: "#FFFFFF".into(), background: None },
                start: Ticks::ZERO,
                duration: Ticks::from_secs(2.0),
                track_id: None,
            },
        )
        .unwrap();
        let overlay = p.tracks.iter().find(|t| t.kind == TrackKind::Overlay).unwrap().id.clone();
        let locked = crate::apply(&p, &EditOp::SetTrack { track_id: overlay.clone(), name: None, locked: Some(true), muted: None, hidden: None, volume_db: None }).unwrap();
        assert!(matches!(crate::apply(&locked, &EditOp::RemoveTrack { track_id: overlay.clone() }), Err(TimelineError::TrackLocked { .. })));
        let p = crate::apply(&p, &EditOp::RemoveTrack { track_id: overlay.clone() }).unwrap();
        assert!(p.tracks.iter().all(|t| t.id != overlay));
        assert!(p.clips.iter().all(|c| c.track_id != overlay));
    }
}
