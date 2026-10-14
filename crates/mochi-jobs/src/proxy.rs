//! S03: proxy cho video nặng (lớn hơn 1080p). Proxy chỉ dùng cho preview — xuất luôn đọc file gốc.
//! Bản 720p, keyframe mỗi nửa giây để tua nhanh, không có tiếng (tiếng preview vẫn đọc file gốc).

use ffmpeg::{format, frame, media, software::scaling};
use mochi_export::{VideoCodec, caps, encode::VideoEncoder};
use mochi_project::{MediaItem, MediaKind};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

/// Cạnh dài tối đa của proxy.
pub const PROXY_LONG_SIDE: u32 = 1280;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProxyError {
    #[error("đã tạm hoãn")]
    Cancelled,
    #[error("file không có luồng hình")]
    NoVideo,
    #[error("{0}")]
    Failed(String),
}

fn fail(e: impl std::fmt::Display) -> ProxyError {
    ProxyError::Failed(e.to_string())
}

/// Media nào cần proxy: video hoặc bản quay lớn hơn 1920×1080.
pub fn needs_proxy(m: &MediaItem) -> bool {
    matches!(m.kind, MediaKind::Video | MediaKind::Recording)
        && m.video.is_some_and(|v| v.width as u64 * v.height as u64 > 1920 * 1080)
}

/// S03: proxy của một bản quay (thư mục layout Cap). Chỉ đổi luồng hình màn hình sang 720p; webcam,
/// tiếng, con trỏ giữ nguyên bằng hard link (cùng ổ, không tốn chỗ; khác ổ thì chép). Renderer Cap đọc
/// thư mục proxy y như bản gốc (tọa độ con trỏ / zoom đều chuẩn hóa 0–1).
pub fn make_recording(src: &Path, dst: &Path, cancel: &AtomicBool, mut progress: impl FnMut(f32)) -> Result<(), ProxyError> {
    let meta = cap_project::RecordingMeta::load_for_project(src).map_err(fail)?;
    let displays: Vec<std::path::PathBuf> = match meta.studio_meta().ok_or_else(|| fail("không phải bản quay studio"))? {
        cap_project::StudioRecordingMeta::SingleSegment { segment } => vec![segment.display.path.to_path(src)],
        cap_project::StudioRecordingMeta::MultipleSegments { inner } => {
            inner.segments.iter().map(|s| s.display.path.to_path(src)).collect()
        }
    };
    if displays.iter().any(|d| !d.is_file()) {
        return Err(fail("bản quay chưa được đóng gói (segment dạng thư mục)"));
    }
    let _ = std::fs::remove_dir_all(dst);
    std::fs::create_dir_all(dst).map_err(fail)?;
    link_tree(src, src, dst, &displays).map_err(fail)?;
    let n = displays.len() as f32;
    for (i, d) in displays.iter().enumerate() {
        let rel = d.strip_prefix(src).map_err(fail)?;
        let out = dst.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(fail)?;
        }
        make(d, &out, cancel, |p| progress((i as f32 + p) / n))?;
    }
    // Dung lượng thật của proxy (chỉ phần hình 720p; file hard link dùng chung với bản gốc) cho cache.
    let own: u64 = displays
        .iter()
        .filter_map(|d| d.strip_prefix(src).ok())
        .filter_map(|rel| std::fs::metadata(dst.join(rel)).ok())
        .map(|m| m.len())
        .sum();
    std::fs::write(dst.join(PROXY_BYTES_FILE), own.to_string()).map_err(fail)?;
    progress(1.0);
    Ok(())
}

/// Trong thư mục proxy của bản quay: số byte proxy thật sự chiếm (để tính cache).
pub const PROXY_BYTES_FILE: &str = ".mochi-proxy-bytes";

/// Hard link (lùi về chép) mọi file của `dir` sang `dst`, trừ các file hình màn hình (`skip`) và các
/// thư mục làm việc `.recovery-*`.
fn link_tree(root: &Path, dir: &Path, dst: &Path, skip: &[std::path::PathBuf]) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with(".recovery-") {
            continue;
        }
        let out = dst.join(p.strip_prefix(root).unwrap_or(&p));
        if p.is_dir() {
            std::fs::create_dir_all(&out)?;
            link_tree(root, &p, dst, skip)?;
        } else if !skip.contains(&p) {
            if std::fs::hard_link(&p, &out).is_err() {
                std::fs::copy(&p, &out)?;
            }
        }
    }
    Ok(())
}

fn even(v: f64) -> u32 {
    ((v / 2.0).round() as u32 * 2).max(2)
}

/// Tạo proxy `dst` từ `src`. `cancel` bật → dừng giữa chừng (`Cancelled`), người gọi xóa file dở.
pub fn make(src: &Path, dst: &Path, cancel: &AtomicBool, mut progress: impl FnMut(f32)) -> Result<(), ProxyError> {
    let _ = ffmpeg::init();
    let mut ictx = format::input(src).map_err(fail)?;
    let st = ictx.streams().best(media::Type::Video).ok_or(ProxyError::NoVideo)?;
    let idx = st.index();
    let tb = st.time_base();
    let start = match st.start_time() {
        t if t == ffmpeg::ffi::AV_NOPTS_VALUE => 0,
        t => t,
    };
    let rate = st.avg_frame_rate();
    let fps = if rate.denominator() > 0 && rate.numerator() > 0 {
        (rate.numerator() as f64 / rate.denominator() as f64).round().clamp(1.0, 120.0) as u32
    } else {
        30
    };
    let duration = (ictx.duration().max(1) as f64 / ffmpeg::ffi::AV_TIME_BASE as f64).max(0.001);
    let mut dec_ctx = ffmpeg::codec::context::Context::from_parameters(st.parameters()).map_err(fail)?;
    dec_ctx.set_threading(ffmpeg::codec::threading::Config::count(
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
    ));
    let mut dec = dec_ctx.decoder().video().map_err(fail)?;

    let (sw, sh) = (dec.width().max(2) as f64, dec.height().max(2) as f64);
    let s = (PROXY_LONG_SIDE as f64 / sw.max(sh)).min(1.0);
    let (w, h) = (even(sw * s), even(sh * s));

    let encoder = caps::support_for(VideoCodec::H264)
        .and_then(|c| c.encoder.clone())
        .ok_or_else(|| fail("không có encoder H.264"))?;
    let mut out = format::output_as(dst, "mp4").map_err(fail)?;
    let bitrate = (w as f64 * h as f64 * fps as f64 * 0.12) as u64;
    let mut enc = VideoEncoder::open_with_gop(&mut out, &encoder, w, h, fps, bitrate, (fps / 2).max(1)).map_err(fail)?;
    out.write_header().map_err(fail)?;

    let mut scaler: Option<scaling::Context> = None;
    let mut rgba = frame::Video::empty();
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut last: i64 = -1;
    let mut decoded = frame::Video::empty();

    let mut handle = |frame: &frame::Video,
                      scaler: &mut Option<scaling::Context>,
                      out: &mut format::context::Output|
     -> Result<(), ProxyError> {
        let ts = frame.timestamp().or(frame.pts()).unwrap_or(0) - start;
        let secs = ts as f64 * tb.numerator() as f64 / tb.denominator().max(1) as f64;
        let index = (secs * fps as f64).round() as i64;
        if index <= last {
            return Ok(()); // VFR: hai frame rơi vào cùng một ô thời gian của proxy
        }
        // Khung hỏng không rõ định dạng → swscale assert làm sập cả tiến trình: báo lỗi thay vì vậy.
        if frame.format() == format::Pixel::None || frame.width() == 0 || frame.height() == 0 {
            return Err(fail("khung hình hỏng"));
        }
        if scaler.as_ref().is_none_or(|c| c.input().format != frame.format() || c.input().width != frame.width()) {
            *scaler = Some(
                scaling::Context::get(frame.format(), frame.width(), frame.height(), format::Pixel::RGBA, w, h, scaling::Flags::BILINEAR)
                    .map_err(fail)?,
            );
        }
        scaler.as_mut().unwrap().run(frame, &mut rgba).map_err(fail)?;
        let stride = rgba.stride(0);
        let data = rgba.data(0);
        for row in 0..h as usize {
            buf[row * w as usize * 4..(row + 1) * w as usize * 4].copy_from_slice(&data[row * stride..row * stride + w as usize * 4]);
        }
        enc.push_rgba(&buf, index, out).map_err(fail)?;
        last = index;
        progress((secs / duration).clamp(0.0, 1.0) as f32);
        Ok(())
    };

    for (stream, packet) in ictx.packets() {
        if cancel.load(Ordering::SeqCst) {
            return Err(ProxyError::Cancelled);
        }
        if stream.index() != idx {
            continue;
        }
        dec.send_packet(&packet).map_err(fail)?;
        while dec.receive_frame(&mut decoded).is_ok() {
            handle(&decoded, &mut scaler, &mut out)?;
        }
    }
    dec.send_eof().map_err(fail)?;
    while dec.receive_frame(&mut decoded).is_ok() {
        handle(&decoded, &mut scaler, &mut out)?;
    }
    drop(handle);
    enc.finish(&mut out).map_err(fail)?;
    out.write_trailer().map_err(fail)?;
    progress(1.0);
    Ok(())
}
