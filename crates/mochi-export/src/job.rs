//! Chạy một lần xuất (bên trong tiến trình con): render từng frame bằng `Renderer` dùng chung với
//! preview → encoder; tiếng trộn bằng `OfflineMixer` (cùng hàm trộn với preview).

use crate::caps;
use crate::encode::VideoEncoder;
use crate::error::ExportError;
use crate::preflight::estimate_bytes;
use crate::settings::*;
use cap_enc_ffmpeg::aac::AACEncoder;
use cap_media_info::AudioInfo;
use mochi_preview::OfflineMixer;
use mochi_project::{Project, Ticks};
use mochi_render::{GpuContext, Renderer, read_texture_rgba};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Mọi thứ tiến trình con cần: ảnh chụp project lúc bấm xuất (người dùng sửa tiếp không ảnh hưởng).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJob {
    pub id: String,
    pub project: Project,
    pub paths: HashMap<String, PathBuf>,
    pub settings: ExportSettings,
    /// Đường dẫn đã chuẩn hóa (từ preflight).
    pub output: PathBuf,
    pub log: PathBuf,
}

/// Tin nhắn từ tiến trình con (mỗi dòng stdout một JSON).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChildMsg {
    #[serde(rename_all = "camelCase")]
    Started { encoder: String, hardware: bool, width: u32, height: u32, frames: u32 },
    #[serde(rename_all = "camelCase")]
    Progress { frame: u32, frames: u32, fps: f32, eta_secs: f32 },
    /// Encoder phần cứng lỗi giữa chừng → chuyển sang phần mềm và render lại từ đầu.
    #[serde(rename_all = "camelCase")]
    Fallback { from: String, to: String },
    #[serde(rename_all = "camelCase")]
    Done {
        path: PathBuf,
        #[specta(type = f64)]
        bytes: u64,
        secs: f32,
        encoder: String,
    },
    #[serde(rename_all = "camelCase")]
    Failed { error: ExportError },
}

/// Biến môi trường kiểm thử (chỉ dùng trong kịch bản `mochi-cli`):
/// - `MOCHI_TEST_EXPORT_HW_FAIL_AT=<frame>`: encoder phần cứng "lỗi" ở frame đó;
/// - `MOCHI_TEST_EXPORT_FREE_BYTES=<frame>:<bytes>`: từ frame đó coi như ổ chỉ còn `<bytes>`;
/// - `MOCHI_TEST_EXPORT_CRASH_AT=<frame>`: tiến trình con tự thoát đột ngột ở frame đó.
fn test_env_u64(name: &str) -> Option<u64> {
    mochi_project::test_env(name)?.parse().ok()
}

fn free_bytes_at(dir: &Path, frame: u32) -> u64 {
    if let Some(v) = mochi_project::test_env("MOCHI_TEST_EXPORT_FREE_BYTES")
        && let Some((at, bytes)) = v.split_once(':')
        && let (Ok(at), Ok(bytes)) = (at.parse::<u32>(), bytes.parse::<u64>())
        && frame >= at
    {
        return bytes;
    }
    mochi_recorder::disk::free_bytes(dir).unwrap_or(u64::MAX)
}

/// Dưới mức này thì dừng xuất (để còn chỗ cho trailer + hệ thống).
const MIN_FREE_WHILE_EXPORTING: u64 = 64 * 1024 * 1024;

struct Ctx<'a> {
    job: &'a ExportJob,
    cancel: &'a AtomicBool,
    on_msg: &'a mut dyn FnMut(ChildMsg),
    renderer: Renderer,
    gpu: GpuContext,
    width: u32,
    height: u32,
    range: mochi_project::TimeRange,
    frames: u32,
}

/// Chạy job tới khi xong. File tạm bị xóa ở mọi nhánh lỗi / hủy.
pub async fn run(job: &ExportJob, cancel: &AtomicBool, on_msg: &mut dyn FnMut(ChildMsg)) -> Result<ChildMsg, ExportError> {
    let _ = ffmpeg::init();
    let s = &job.settings;
    let range = export_range(&job.project, s);
    let (width, height) = output_size(&job.project, s.short_side);
    let frames = frame_count(&range, s.fps);
    let gpu = GpuContext::new().map_err(ExportError::internal)?;
    let mut ctx = Ctx { job, cancel, on_msg, renderer: Renderer::new(gpu.clone()), gpu, width, height, range, frames };
    let partial = partial_path(&job.output);
    let started = Instant::now();

    let result = match s.format {
        ExportFormat::Gif => ctx.gif(&partial).await.map(|()| "gifski".to_string()),
        ExportFormat::Mp4 => ctx.mp4_with_fallback(&partial).await,
        ExportFormat::M4a => ctx.audio_only(&partial, false).await,
        ExportFormat::Mp3 => ctx.audio_only(&partial, true).await,
    };
    let encoder = match result {
        Ok(e) => e,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };
    if job.output.exists() {
        if !s.overwrite {
            let _ = std::fs::remove_file(&partial);
            return Err(ExportError::FileExists { path: job.output.clone() });
        }
        std::fs::remove_file(&job.output).map_err(ExportError::internal)?;
    }
    std::fs::rename(&partial, &job.output).map_err(ExportError::internal)?;
    let bytes = std::fs::metadata(&job.output).map(|m| m.len()).unwrap_or(0);
    Ok(ChildMsg::Done { path: job.output.clone(), bytes, secs: started.elapsed().as_secs_f32(), encoder })
}

impl Ctx<'_> {
    fn check(&self, frame: u32, dir: &Path, written: u64) -> Result<(), ExportError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(ExportError::Cancelled);
        }
        if let Some(at) = test_env_u64("MOCHI_TEST_EXPORT_CRASH_AT")
            && frame as u64 == at
        {
            tracing::error!("MOCHI_TEST_EXPORT_CRASH_AT: tự thoát đột ngột ở frame {frame}");
            std::process::exit(101);
        }
        if frame % 15 == 0 {
            let free = free_bytes_at(dir, frame);
            if free < MIN_FREE_WHILE_EXPORTING {
                let est = estimate_bytes(&self.job.settings, self.width, self.height, self.frames, true);
                let needed = est.saturating_sub(written) + MIN_FREE_WHILE_EXPORTING;
                return Err(ExportError::DiskFull { needed_bytes: needed, free_bytes: free });
            }
        }
        Ok(())
    }

    /// Lỗi ghi khi ổ đã gần đầy → `DiskFull` (kèm số byte còn cần) thay cho lỗi encoder chung chung.
    fn disk_full_instead(&self, partial: &Path) -> Option<ExportError> {
        let dir = partial.parent().unwrap_or(Path::new("."));
        let free = free_bytes_at(dir, u32::MAX);
        (free < MIN_FREE_WHILE_EXPORTING).then(|| {
            let written = std::fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
            let est = estimate_bytes(&self.job.settings, self.width, self.height, self.frames, true);
            ExportError::DiskFull { needed_bytes: est.saturating_sub(written) + MIN_FREE_WHILE_EXPORTING, free_bytes: free }
        })
    }

    async fn frame_rgba(&mut self, i: u32) -> Vec<u8> {
        let t = frame_time(&self.range, self.job.settings.fps, i);
        let (tex, _) = self
            .renderer
            .render_sized(&self.job.project, &self.job.paths, t, true, Some((self.width, self.height)))
            .await;
        read_texture_rgba(&self.gpu, &tex)
    }

    fn progress(&mut self, frame: u32, started: Instant, last: &mut Instant) {
        if last.elapsed() < Duration::from_millis(250) && frame + 1 != self.frames {
            return;
        }
        *last = Instant::now();
        let fps = (frame + 1) as f32 / started.elapsed().as_secs_f32().max(0.001);
        let eta = (self.frames - frame - 1) as f32 / fps.max(0.001);
        (self.on_msg)(ChildMsg::Progress { frame: frame + 1, frames: self.frames, fps, eta_secs: eta });
    }

    async fn mp4_with_fallback(&mut self, partial: &Path) -> Result<String, ExportError> {
        let support = caps::support_for(self.job.settings.codec);
        let working = support.map(|c| c.working.clone()).unwrap_or_default();
        let first = working.first().cloned().ok_or_else(|| ExportError::UnsupportedEncoder {
            codec: self.job.settings.codec,
            reason: support.and_then(|c| c.reason.clone()).unwrap_or_default(),
        })?;
        let result = self.mp4(partial, &first).await;
        // Ổ đầy giữa hai lần kiểm (mỗi 15 frame): lỗi ghi bị gói thành lỗi encoder. Kiểm ổ trước khi
        // coi là lỗi encoder → báo hết dung lượng, không chuyển nhầm sang libx264 rồi render lại.
        let result = match result {
            Err(e @ (ExportError::Encoder { .. } | ExportError::Internal { .. })) => Err(self.disk_full_instead(partial).unwrap_or(e)),
            other => other,
        };
        match result {
            Err(ExportError::Encoder { message, hardware: true }) => {
                // Encoder phần cứng lỗi giữa chừng → phần mềm (H.264), render lại từ đầu.
                tracing::warn!("encoder {first} lỗi: {message} → chuyển sang libx264");
                let _ = std::fs::remove_file(partial);
                (self.on_msg)(ChildMsg::Fallback { from: first, to: "libx264".into() });
                self.renderer = Renderer::new(self.gpu.clone());
                self.mp4(partial, "libx264").await?;
                Ok("libx264".into())
            }
            Err(e) => Err(e),
            Ok(()) => Ok(first),
        }
    }

    async fn mp4(&mut self, partial: &Path, encoder: &str) -> Result<(), ExportError> {
        let s = self.job.settings.clone();
        let (w, h) = (self.width, self.height);
        let dir = partial.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mixer = OfflineMixer::new(Arc::new(self.job.project.clone()), &self.job.paths);
        let has_audio = mixer.has_audio();

        let mut out = ffmpeg::format::output_as(partial, "mp4").map_err(|e| ExportError::NotWritable { dir: dir.clone() }.log(e))?;
        let bitrate = video_bitrate(w, h, s.fps, s.quality, s.codec);
        let mut video = VideoEncoder::open(&mut out, encoder, w, h, s.fps, bitrate)
            .map_err(|message| ExportError::Encoder { message, hardware: caps::is_hardware(encoder) })?;
        let audio_info = AudioInfo::new(
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            OfflineMixer::RATE,
            2,
        )
        .map_err(ExportError::internal)?;
        let mut aac = if has_audio { Some(AACEncoder::init(audio_info, &mut out).map_err(ExportError::internal)?) } else { None };
        let mut opts = ffmpeg::Dictionary::new();
        opts.set("movflags", "+faststart");
        out.write_header_with(opts).map_err(ExportError::internal)?;

        (self.on_msg)(ChildMsg::Started { encoder: encoder.into(), hardware: video.hardware, width: w, height: h, frames: self.frames });
        tracing::info!("xuất MP4 {w}×{h} @{} fps, {} frame, encoder {encoder}, tiếng: {has_audio}", s.fps, self.frames);

        let hw_fail_at = test_env_u64("MOCHI_TEST_EXPORT_HW_FAIL_AT");
        let rate = OfflineMixer::RATE as u64;
        let mut pcm = Vec::new();
        let started = Instant::now();
        let mut last = Instant::now() - Duration::from_secs(1);
        for i in 0..self.frames {
            let written = std::fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
            self.check(i, &dir, written)?;
            let rgba = self.frame_rgba(i).await;
            if video.hardware && hw_fail_at == Some(i as u64) {
                return Err(ExportError::Encoder { message: "MOCHI_TEST_EXPORT_HW_FAIL_AT".into(), hardware: true });
            }
            video
                .push_rgba(&rgba, i as i64, &mut out)
                .map_err(|message| ExportError::Encoder { message, hardware: video.hardware })?;

            if let Some(aac) = aac.as_mut() {
                // Đúng số mẫu của frame này: tổng số mẫu luôn khớp thời lượng hình (S05).
                let s0 = i as u64 * rate / s.fps as u64;
                let s1 = (i as u64 + 1) * rate / s.fps as u64;
                let n = (s1 - s0) as usize;
                pcm.resize(n * 2, 0.0);
                let t = Ticks(self.range.start.0 + s0 as i64 * OfflineMixer::TICKS_PER_SAMPLE);
                mixer.fill(t, &mut pcm);
                let mut frame = ffmpeg::frame::Audio::new(
                    ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                    n,
                    ffmpeg::ChannelLayout::STEREO,
                );
                frame.set_rate(OfflineMixer::RATE);
                for (dst, v) in frame.data_mut(0).chunks_exact_mut(4).zip(&pcm) {
                    dst.copy_from_slice(&v.to_ne_bytes());
                }
                aac.send_frame(frame, Duration::from_secs_f64(s0 as f64 / rate as f64), &mut out)
                    .map_err(ExportError::internal)?;
            }
            self.progress(i, started, &mut last);
        }
        video.finish(&mut out).map_err(|message| ExportError::Encoder { message, hardware: video.hardware })?;
        if let Some(aac) = aac.as_mut() {
            aac.flush(&mut out).map_err(ExportError::internal)?;
        }
        out.write_trailer().map_err(ExportError::internal)?;
        Ok(())
    }

    /// X02 (v1): chỉ xuất âm thanh — cùng bộ trộn với MP4 / preview (OfflineMixer).
    async fn audio_only(&mut self, partial: &Path, mp3: bool) -> Result<String, ExportError> {
        use ffmpeg::format::{Sample, sample::Type};
        let dir = partial.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mixer = OfflineMixer::new(Arc::new(self.job.project.clone()), &self.job.paths);
        let (fmt, codec_name) = if mp3 { ("mp3", "libmp3lame") } else { ("ipod", "aac") };
        let mut out = ffmpeg::format::output_as(partial, fmt).map_err(|e| ExportError::NotWritable { dir: dir.clone() }.log(e))?;
        let codec = ffmpeg::encoder::find_by_name(codec_name)
            .ok_or_else(|| ExportError::internal(format!("không có encoder {codec_name}")))?;
        let global = out.format().flags().contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let rate = OfflineMixer::RATE;
        let mut enc = ffmpeg::codec::context::Context::new_with_codec(codec).encoder().audio().map_err(ExportError::internal)?;
        enc.set_rate(rate as i32);
        enc.set_channel_layout(ffmpeg::ChannelLayout::STEREO);
        enc.set_format(Sample::F32(Type::Planar));
        enc.set_bit_rate(crate::preflight::audio_bitrate(self.job.settings.quality) as usize);
        enc.set_time_base((1, rate as i32));
        if global {
            enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }
        let mut enc = enc.open_as(codec).map_err(ExportError::internal)?;
        let stream_index = {
            let mut st = out.add_stream(codec).map_err(ExportError::internal)?;
            st.set_parameters(&enc);
            st.set_time_base((1, rate as i32));
            st.index()
        };
        out.write_header().map_err(ExportError::internal)?;
        let stream_tb = out.stream(stream_index).map(|s| s.time_base()).unwrap_or((1, rate as i32).into());
        let encoder = codec_name.to_string();
        (self.on_msg)(ChildMsg::Started { encoder: encoder.clone(), hardware: false, width: 0, height: 0, frames: self.frames });

        let total = ((self.range.end.0 - self.range.start.0) / OfflineMixer::TICKS_PER_SAMPLE).max(0) as u64;
        let chunk = enc.frame_size().max(1024) as u64;
        let mut pcm = Vec::new();
        let started = Instant::now();
        let mut last = Instant::now() - Duration::from_secs(1);
        let mut write_packets = |enc: &mut ffmpeg::encoder::Audio, out: &mut ffmpeg::format::context::Output| -> Result<(), ExportError> {
            let mut pkt = ffmpeg::Packet::empty();
            while enc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(stream_index);
                pkt.rescale_ts((1, rate as i32), stream_tb);
                pkt.write_interleaved(out).map_err(ExportError::internal)?;
            }
            Ok(())
        };
        let mut s0 = 0u64;
        while s0 < total {
            let n = chunk.min(total - s0) as usize;
            let frame_i = (s0 * self.job.settings.fps as u64 / rate as u64) as u32;
            self.check(frame_i, &dir, std::fs::metadata(partial).map(|m| m.len()).unwrap_or(0))?;
            pcm.resize(n * 2, 0.0);
            mixer.fill(Ticks(self.range.start.0 + s0 as i64 * OfflineMixer::TICKS_PER_SAMPLE), &mut pcm);
            let mut frame = ffmpeg::frame::Audio::new(Sample::F32(Type::Planar), chunk as usize, ffmpeg::ChannelLayout::STEREO);
            frame.set_rate(rate);
            frame.set_samples(n);
            for ch in 0..2 {
                let plane = frame.data_mut(ch);
                for (k, dst) in plane.chunks_exact_mut(4).take(n).enumerate() {
                    dst.copy_from_slice(&pcm[k * 2 + ch].to_ne_bytes());
                }
            }
            frame.set_pts(Some(s0 as i64));
            enc.send_frame(&frame).map_err(ExportError::internal)?;
            write_packets(&mut enc, &mut out)?;
            s0 += n as u64;
            self.progress(frame_i.min(self.frames.saturating_sub(1)), started, &mut last);
        }
        enc.send_eof().map_err(ExportError::internal)?;
        write_packets(&mut enc, &mut out)?;
        out.write_trailer().map_err(ExportError::internal)?;
        Ok(encoder)
    }

    async fn gif(&mut self, partial: &Path) -> Result<(), ExportError> {
        let s = self.job.settings.clone();
        let (w, h) = (self.width, self.height);
        let dir = partial.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut gif = cap_enc_gif::GifEncoderWrapper::new_with_quality(
            partial,
            w,
            h,
            s.fps,
            cap_enc_gif::GifQuality { quality: s.quality.gif_quality(), fast: false },
        )
        .map_err(|e| ExportError::NotWritable { dir: dir.clone() }.log(e))?;
        (self.on_msg)(ChildMsg::Started { encoder: "gifski".into(), hardware: false, width: w, height: h, frames: self.frames });
        let started = Instant::now();
        let mut last = Instant::now() - Duration::from_secs(1);
        for i in 0..self.frames {
            self.check(i, &dir, 0)?;
            let rgba = self.frame_rgba(i).await;
            gif.add_frame(&rgba, w as usize * 4).map_err(ExportError::internal)?;
            self.progress(i, started, &mut last);
        }
        gif.finish().map_err(ExportError::internal)?;
        Ok(())
    }
}

impl ExportError {
    fn log(self, e: impl std::fmt::Display) -> Self {
        tracing::error!("{self}: {e}");
        self
    }
}
