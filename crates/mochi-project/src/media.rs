//! Import media (E01): dò thông tin bằng FFmpeg, hash để phát hiện trùng, relink khi thiếu.
//! Media gốc chỉ được MỞ ĐỂ ĐỌC (K-E1).

use crate::{
    AudioInfo, MediaItem, MediaKind, ProjectError, RecordingInfo, Ticks, VideoInfo,
    package::RECORDINGS_DIR,
};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub const VIDEO_EXT: &[&str] = &["mp4", "mov", "mkv", "webm", "m4v", "avi"];
pub const AUDIO_EXT: &[&str] = &["mp3", "wav", "m4a", "aac", "ogg", "flac", "opus"];
pub const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp"];

/// Ảnh tĩnh mặc định dài 5 giây khi thả lên timeline.
pub const IMAGE_DEFAULT_SECS: f64 = 5.0;

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

/// xxh3 của (kích thước + 1 MB đầu + 1 MB cuối): nhanh với file lớn, đủ để nhận ra file trùng.
pub fn quick_hash(path: &Path) -> std::io::Result<(String, u64)> {
    const CHUNK: u64 = 1024 * 1024;
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    hasher.update(&size.to_le_bytes());
    let mut buf = vec![0u8; CHUNK as usize];
    let n = f.read(&mut buf)?;
    hasher.update(&buf[..n]);
    if size > CHUNK * 2 {
        f.seek(SeekFrom::End(-(CHUNK as i64)))?;
        let n = f.read(&mut buf)?;
        hasher.update(&buf[..n]);
    }
    Ok((format!("{:016x}", hasher.digest()), size))
}

/// Dò một file media. File hỏng / codec không hỗ trợ → `Unsupported` kèm lý do.
pub fn probe(path: &Path) -> Result<MediaItem, ProjectError> {
    // Luôn lưu đường dẫn tuyệt đối: đường dẫn tương đối phụ thuộc thư mục đang chạy của app.
    let absolute = std::path::absolute(path)?;
    let path = absolute.as_path();
    if !path.is_file() {
        return Err(ProjectError::NotFound);
    }
    let ext = ext_of(path);
    let (hash, size) = quick_hash(path)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let base = |kind, duration, video, audio| MediaItem {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        name: name.clone(),
        abs_path: path.to_path_buf(),
        rel_path: None,
        hash: hash.clone(),
        size_bytes: size as f64,
        duration,
        video,
        audio,
        recording: None,
    };

    if IMAGE_EXT.contains(&ext.as_str()) {
        let (w, h) = image::image_dimensions(path).map_err(|e| ProjectError::Unsupported {
            detail: format!("ảnh không đọc được: {e}"),
        })?;
        // E01: GIF động → độ dài mặc định = một vòng hoạt ảnh (renderer phát lặp lại khi kéo dài clip).
        let anim = (ext == "gif").then(|| gif_animation_secs(path)).flatten();
        return Ok(base(
            MediaKind::Image,
            Ticks::from_secs(anim.unwrap_or(IMAGE_DEFAULT_SECS)),
            Some(VideoInfo { width: w, height: h, fps: anim.map_or(0.0, |_| 1.0) }),
            None,
        ));
    }
    if !VIDEO_EXT.contains(&ext.as_str()) && !AUDIO_EXT.contains(&ext.as_str()) {
        return Err(ProjectError::Unsupported {
            detail: format!("định dạng .{ext} chưa hỗ trợ"),
        });
    }

    let (duration, video, audio) = probe_av(path)?;
    let kind = if video.is_some() {
        MediaKind::Video
    } else if audio.is_some() {
        MediaKind::Audio
    } else {
        return Err(ProjectError::Unsupported {
            detail: "không có luồng hình hay tiếng".into(),
        });
    };
    if duration <= Ticks::ZERO {
        return Err(ProjectError::Unsupported {
            detail: "không đọc được thời lượng".into(),
        });
    }
    Ok(base(kind, duration, video, audio))
}

/// Tổng thời gian một vòng của GIF động (giây); None nếu GIF chỉ có một khung.
pub fn gif_animation_secs(path: &Path) -> Option<f64> {
    let frames = gif_frames(path, 1).ok()?;
    (frames.len() > 1).then(|| frames.iter().map(|(_, d)| d).sum::<f64>().max(0.1))
}

/// Giải mã GIF: (khung RGBA, thời gian hiển thị giây). Khung có delay 0 hiển thị 0,1 s như trình duyệt.
/// `max_frames_kept` = 0: giữ ảnh mọi khung; n: chỉ giữ n khung đầu (dò nhanh lúc import).
pub fn gif_frames(path: &Path, max_frames_kept: usize) -> Result<Vec<(Option<image::RgbaImage>, f64)>, String> {
    use image::AnimationDecoder;
    let file = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    let decoder = image::codecs::gif::GifDecoder::new(file).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (i, frame) in decoder.into_frames().enumerate() {
        let frame = frame.map_err(|e| e.to_string())?;
        let (n, d) = frame.delay().numer_denom_ms();
        let secs = if n == 0 { 0.1 } else { (n as f64 / d.max(1) as f64 / 1000.0).max(0.02) };
        let keep = max_frames_kept == 0 || i < max_frames_kept;
        out.push((keep.then(|| frame.into_buffer()), secs));
        if out.len() >= 2000 {
            break; // GIF quá dài: giữ 2000 khung đầu
        }
    }
    Ok(out)
}

fn probe_av(path: &Path) -> Result<(Ticks, Option<VideoInfo>, Option<AudioInfo>), ProjectError> {
    use ffmpeg::media::Type;
    ffmpeg::init().map_err(|e| ProjectError::Unsupported { detail: e.to_string() })?;
    let unsupported = |e: ffmpeg::Error| ProjectError::Unsupported {
        detail: format!("FFmpeg: {e}"),
    };
    let ictx = ffmpeg::format::input(&path).map_err(unsupported)?;
    let mut duration = if ictx.duration() > 0 {
        Ticks::from_secs(ictx.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE))
    } else {
        Ticks::ZERO
    };

    let video = match ictx.streams().best(Type::Video) {
        Some(stream) => {
            let ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(unsupported)?;
            let dec = ctx.decoder().video().map_err(unsupported)?;
            let rate = stream.avg_frame_rate();
            let fps = if rate.denominator() != 0 {
                f64::from(rate.numerator()) / f64::from(rate.denominator())
            } else {
                0.0
            };
            if duration == Ticks::ZERO && stream.duration() > 0 {
                let tb = stream.time_base();
                duration = Ticks::from_secs(
                    stream.duration() as f64 * f64::from(tb.numerator()) / f64::from(tb.denominator()),
                );
            }
            Some(VideoInfo { width: dec.width(), height: dec.height(), fps })
        }
        None => None,
    };
    let audio = match ictx.streams().best(Type::Audio) {
        Some(stream) => {
            let ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .map_err(unsupported)?;
            let dec = ctx.decoder().audio().map_err(unsupported)?;
            Some(AudioInfo {
                channels: u32::from(dec.channels()),
                sample_rate: dec.rate(),
            })
        }
        None => None,
    };
    Ok((duration, video, audio))
}

/// Media "bản quay" từ một thư mục `recordings/<id>` (layout Cap): các segment nối liền nhau.
pub fn probe_recording(project_dir: &Path, recording_dir: &Path) -> Result<MediaItem, ProjectError> {
    let meta = cap_project::RecordingMeta::load_for_project(recording_dir).map_err(|e| {
        ProjectError::Unsupported {
            detail: format!("recording-meta.json: {e}"),
        }
    })?;
    let studio = meta.studio_meta().ok_or_else(|| ProjectError::Unsupported {
        detail: "không phải bản quay studio".into(),
    })?;
    let mut segments = Vec::new();
    let (mut has_cursor, mut has_camera, mut has_mic, mut has_sys) = (false, false, false, false);
    let mut video = None;
    match studio {
        cap_project::StudioRecordingMeta::SingleSegment { segment } => {
            let p = meta.path(&segment.display.path);
            let (d, v, _) = probe_av(&p)?;
            segments.push(d);
            video = v;
            has_cursor = segment.cursor.is_some();
            has_camera = segment.camera.is_some();
            has_mic = segment.audio.is_some();
        }
        cap_project::StudioRecordingMeta::MultipleSegments { inner } => {
            for seg in &inner.segments {
                let p = meta.path(&seg.display.path);
                let (d, v, _) = probe_av(&p)?;
                segments.push(d);
                video = video.or(v);
                has_cursor |= seg.cursor.is_some();
                has_camera |= seg.camera.is_some();
                has_mic |= seg.mic.is_some();
                has_sys |= seg.system_audio.is_some();
            }
        }
    }
    let duration = segments.iter().fold(Ticks::ZERO, |a, b| a + *b);
    let sidecar = crate::sidecar::RecordingSidecar::read(recording_dir);
    let rel = recording_dir
        .strip_prefix(project_dir)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    Ok(MediaItem {
        id: uuid::Uuid::new_v4().to_string(),
        kind: crate::MediaKind::Recording,
        name: meta.pretty_name.clone(),
        abs_path: recording_dir.to_path_buf(),
        rel_path: rel,
        hash: format!(
            "rec-{}",
            recording_dir.file_name().and_then(|n| n.to_str()).unwrap_or_default()
        ),
        size_bytes: 0.0,
        duration,
        video,
        audio: Some(AudioInfo { channels: 2, sample_rate: 48_000 }),
        recording: Some(RecordingInfo {
            segments,
            has_cursor,
            has_camera,
            has_mic,
            has_system_audio: has_sys,
            lost: sidecar.lost,
            active_windows: sidecar.active_windows,
        }),
    })
}

/// Liệt kê các bản quay trong gói (`recordings/*`).
pub fn recordings_in(project_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(project_dir.join(RECORDINGS_DIR)) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("recording-meta.json").is_file())
        .collect();
    v.sort();
    v
}

/// Đường dẫn thật hiện tại của media: ưu tiên tương đối trong gói (chuyển máy được),
/// rồi tới tuyệt đối. None = thiếu media.
pub fn resolve(project_dir: &Path, item: &MediaItem) -> Option<PathBuf> {
    let exists = |p: &Path| match item.kind {
        MediaKind::Recording => p.join("recording-meta.json").is_file(),
        _ => p.is_file(),
    };
    if let Some(rel) = &item.rel_path {
        let p = project_dir.join(rel);
        if exists(&p) {
            return Some(p);
        }
    }
    exists(&item.abs_path).then(|| item.abs_path.clone())
}

/// Tìm lại media thiếu theo TÊN trong `folder` (đệ quy, tối đa 4 cấp).
/// Nhiều file cùng tên → ưu tiên cùng hash, rồi cùng kích thước; chỉ một file cùng tên thì nhận
/// luôn (người dùng đã chọn thư mục). Bản quay: thư mục cùng tên có `recording-meta.json`.
pub fn find_relink(folder: &Path, item: &MediaItem) -> Option<PathBuf> {
    let name = match item.kind {
        MediaKind::Recording => item.abs_path.file_name().and_then(|n| n.to_str()).map(str::to_string),
        _ => Some(item.name.clone()),
    }?;
    fn walk(dir: &Path, depth: u32, name: &str, recording: bool, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut subdirs = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            let same = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name));
            if p.is_dir() {
                if recording && same && p.join("recording-meta.json").is_file() {
                    out.push(p.clone());
                }
                subdirs.push(p);
            } else if !recording && same {
                out.push(p);
            }
        }
        if depth > 0 {
            for d in subdirs {
                walk(&d, depth - 1, name, recording, out);
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, 4, &name, item.kind == MediaKind::Recording, &mut found);
    if item.kind == MediaKind::Recording {
        // Nhiều thư mục cùng tên: ưu tiên bản có cùng tên hiển thị (recording-meta.json).
        let same_name = |p: &PathBuf| {
            cap_project::RecordingMeta::load_for_project(p).is_ok_and(|m| m.pretty_name == item.name)
        };
        return found.iter().find(|p| same_name(p)).or(found.first()).cloned();
    }
    if found.len() <= 1 {
        return found.into_iter().next();
    }
    let hashed: Vec<_> = found.iter().map(|p| (p, quick_hash(p).ok())).collect();
    hashed
        .iter()
        .find(|(_, h)| h.as_ref().is_some_and(|(h, _)| *h == item.hash))
        .or_else(|| hashed.iter().find(|(_, h)| h.as_ref().is_some_and(|(_, s)| *s as f64 == item.size_bytes)))
        .or(hashed.first())
        .map(|(p, _)| (*p).clone())
}

/// Ảnh thu nhỏ JPEG (rộng tối đa `max_w`) tại giây `at` của video, hoặc của ảnh tĩnh.
pub fn thumbnail_jpeg(item: &MediaItem, path: &Path, at: f64, max_w: u32) -> Result<Vec<u8>, ProjectError> {
    let err = |e: String| ProjectError::Unsupported { detail: e };
    let rgb: image::RgbImage = match item.kind {
        MediaKind::Image => image::open(path).map_err(|e| err(e.to_string()))?.to_rgb8(),
        MediaKind::Audio => return Err(err("không có hình".into())),
        MediaKind::Video => decode_frame_rgb(path, at)?,
        MediaKind::Recording => {
            let meta = cap_project::RecordingMeta::load_for_project(path).map_err(|e| err(e.to_string()))?;
            let first = match meta.studio_meta().ok_or_else(|| err("không phải bản quay".into()))? {
                cap_project::StudioRecordingMeta::SingleSegment { segment } => meta.path(&segment.display.path),
                cap_project::StudioRecordingMeta::MultipleSegments { inner } => meta.path(
                    &inner.segments.first().ok_or_else(|| err("không có segment".into()))?.display.path,
                ),
            };
            decode_frame_rgb(&first, at)?
        }
    };
    let img = image::DynamicImage::ImageRgb8(rgb).thumbnail(max_w, max_w);
    let mut out = std::io::Cursor::new(Vec::new());
    img.to_rgb8()
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| err(e.to_string()))?;
    Ok(out.into_inner())
}

fn decode_frame_rgb(path: &Path, at: f64) -> Result<image::RgbImage, ProjectError> {
    use ffmpeg::{format::Pixel, media::Type, software::scaling};
    let err = |e: ffmpeg::Error| ProjectError::Unsupported { detail: format!("FFmpeg: {e}") };
    ffmpeg::init().map_err(err)?;
    let mut ictx = ffmpeg::format::input(&path).map_err(err)?;
    let stream = ictx.streams().best(Type::Video).ok_or(ProjectError::Unsupported {
        detail: "không có luồng hình".into(),
    })?;
    let index = stream.index();
    let mut dec = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;
    let target = (at.max(0.0) * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
    let _ = ictx.seek(target, ..target);
    let mut frame = ffmpeg::frame::Video::empty();
    for (s, packet) in ictx.packets() {
        if s.index() != index {
            continue;
        }
        dec.send_packet(&packet).map_err(err)?;
        if dec.receive_frame(&mut frame).is_ok() {
            // Tạo bộ đổi định dạng theo khung ĐÃ giải mã: file hỏng có thể không rõ định dạng điểm
            // ảnh, đưa `Pixel::None` vào swscale là assert của FFmpeg làm sập cả app.
            if frame.format() == Pixel::None || frame.width() == 0 || frame.height() == 0 {
                return Err(ProjectError::Unsupported { detail: "khung hình hỏng".into() });
            }
            let mut scaler = scaling::Context::get(
                frame.format(),
                frame.width(),
                frame.height(),
                Pixel::RGB24,
                frame.width(),
                frame.height(),
                scaling::Flags::BILINEAR,
            )
            .map_err(err)?;
            let mut rgb = ffmpeg::frame::Video::empty();
            scaler.run(&frame, &mut rgb).map_err(err)?;
            let (w, h) = (rgb.width(), rgb.height());
            let stride = rgb.stride(0);
            let data = rgb.data(0);
            let mut buf = Vec::with_capacity((w * h * 3) as usize);
            for row in 0..h as usize {
                buf.extend_from_slice(&data[row * stride..row * stride + w as usize * 3]);
            }
            return image::RgbImage::from_raw(w, h, buf).ok_or(ProjectError::Unsupported {
                detail: "frame rỗng".into(),
            });
        }
    }
    Err(ProjectError::Unsupported { detail: "không giải mã được frame".into() })
}
