//! Nguồn hình cho từng media: video/bản quay giải mã bằng decoder của Cap (Media Foundation
//! phần cứng, lùi về FFmpeg), ảnh tĩnh tải một lần. Mỗi nguồn giữ converter YUV→RGBA riêng
//! nên texture của nó còn hợp lệ cho tới lần convert kế tiếp.

use crate::GpuContext;
use cap_rendering::{
    DecodedFrame, PixelFormat, decoder::AsyncVideoDecoderHandle, yuv_converter::YuvToRgbaConverter,
};
use crate::cap_layer::CapRecordingLayer;
use mochi_project::{MediaItem, MediaKind, RecordingFx, Ticks};
use std::path::{Path, PathBuf};

/// Một frame đã nằm trên GPU (RGBA8).
#[derive(Clone)]
pub struct GpuFrame {
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    /// Phần texture chứa hình (góc trên-trái), tính theo tỉ lệ 0–1. Bộ đổi YUV của Cap cấp texture
    /// ra theo cỡ lớn nhất từng gặp (mặc định 1920×1080), video nhỏ hơn chỉ chiếm góc trên-trái.
    pub uv_max: [f32; 2],
}

impl GpuFrame {
    pub fn whole(view: wgpu::TextureView, width: u32, height: u32) -> Self {
        Self { view, width, height, uv_max: [1.0, 1.0] }
    }
}

pub enum Source {
    Video(VideoSource),
    /// Bản quay vẽ bằng renderer của Cap (nền, zoom, con trỏ, webcam).
    Polished(Box<CapRecordingLayer>),
    /// Dự phòng khi renderer của Cap không mở được: phát như video thường.
    Recording(RecordingSource),
    Image(GpuFrame),
    /// E01: GIF động — mọi khung trên GPU, phát lặp theo thời gian.
    Animated(Vec<(GpuFrame, f64)>),
}

impl Source {
    pub async fn open(gpu: &GpuContext, item: &MediaItem, path: &Path) -> anyhow::Result<Self> {
        Ok(match item.kind {
            MediaKind::Video => {
                let fps = item.video.map_or(30.0, |v| v.fps).round().max(1.0) as u32;
                Source::Video(VideoSource::open(gpu, path.to_path_buf(), fps).await?)
            }
            MediaKind::Recording => match CapRecordingLayer::open(gpu, item, path).await {
                Ok(layer) => Source::Polished(Box::new(layer)),
                Err(e) => {
                    tracing::warn!("renderer Cap lỗi, phát bản quay như video thường: {e:#}");
                    Source::Recording(RecordingSource::open(gpu, item, path).await?)
                }
            },
            MediaKind::Image => match load_animation(gpu, path) {
                Some(frames) => Source::Animated(frames),
                None => Source::Image(load_image(gpu, path)?),
            },
            MediaKind::Audio => anyhow::bail!("media âm thanh không có hình"),
        })
    }

    /// Frame tại thời điểm `t` trong media. `exact` = seek chính xác (khi tua / xuất).
    /// `fx` + `canvas` chỉ dùng cho bản quay đã polish.
    pub async fn frame(
        &mut self,
        gpu: &GpuContext,
        t: Ticks,
        exact: bool,
        fx: Option<&RecordingFx>,
        canvas: (u32, u32),
    ) -> Option<GpuFrame> {
        match self {
            Source::Video(v) => v.frame(gpu, t.secs(), exact).await,
            Source::Polished(layer) => {
                let default_fx = RecordingFx::default();
                layer
                    .frame(fx.unwrap_or(&default_fx), canvas, t)
                    .await
                    .map_err(|e| tracing::debug!("cap layer: {e:#}"))
                    .ok()
            }
            Source::Recording(r) => r.frame(gpu, t, exact).await,
            Source::Image(f) => Some(f.clone()),
            Source::Animated(frames) => {
                let total: f64 = frames.iter().map(|(_, d)| d).sum();
                let mut at = t.secs().max(0.0) % total.max(0.001);
                frames
                    .iter()
                    .find(|(_, d)| {
                        at -= d;
                        at < 0.0
                    })
                    .or(frames.last())
                    .map(|(f, _)| f.clone())
            }
        }
    }

    pub fn is_hardware(&self) -> Option<bool> {
        match self {
            Source::Video(v) => Some(v.decoder.is_hardware_accelerated()),
            Source::Recording(r) => r.segments.first().map(|s| s.decoder.is_hardware_accelerated()),
            Source::Polished(_) => None,
            Source::Image(_) | Source::Animated(_) => None,
        }
    }
}

pub struct VideoSource {
    decoder: AsyncVideoDecoderHandle,
    converter: YuvToRgbaConverter,
    rgba_texture: Option<wgpu::Texture>,
    current: Option<GpuFrame>,
}

impl VideoSource {
    pub async fn open(gpu: &GpuContext, path: PathBuf, fps: u32) -> anyhow::Result<Self> {
        use cap_rendering::decoder::{DecoderType, spawn_decoder};
        // MP4 có B-frame thường kèm edit list dời hình sớm 1–3 frame. Decoder FFmpeg của Cap trừ
        // start_time (đã tính edit list); decoder Media Foundation thì dùng pts thô → hình trễ đúng
        // bằng độ dời. Bù bằng `offset` của Cap khi decoder thực sự là Media Foundation.
        let shift = editlist_shift(&path);
        let mut decoder = spawn_decoder("mochi", path.clone(), fps, shift, false)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        #[cfg(windows)]
        let native = decoder.decoder_type() == DecoderType::MediaFoundation;
        #[cfg(not(windows))]
        let native = false;
        if shift > 0.0 && !native {
            decoder = spawn_decoder("mochi", path.clone(), fps, 0.0, false)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
        }
        if shift > 0.0 {
            tracing::info!("{}: edit list dời {:.0} ms, decoder {:?}", path.display(), shift * 1000.0, decoder.decoder_type());
        }
        Ok(Self {
            decoder,
            converter: YuvToRgbaConverter::new(&gpu.device),
            rgba_texture: None,
            current: None,
        })
    }

    async fn frame(&mut self, gpu: &GpuContext, secs: f64, exact: bool) -> Option<GpuFrame> {
        let decoded = if exact {
            self.decoder.get_frame_initial(secs.max(0.0) as f32).await
        } else {
            self.decoder.get_frame(secs.max(0.0) as f32).await
        };
        match decoded {
            Some(frame) => {
                if let Some(f) = self.upload(gpu, &frame) {
                    self.current = Some(f);
                }
            }
            // Không có frame mới (cuối file / timeout): giữ frame trước để không chớp đen.
            None => tracing::debug!("decoder không trả frame tại {secs:.3}s"),
        }
        self.current.clone()
    }

    fn upload(&mut self, gpu: &GpuContext, frame: &DecodedFrame) -> Option<GpuFrame> {
        let (w, h) = (frame.width(), frame.height());
        let view = match frame.format() {
            PixelFormat::Rgba => {
                let reuse = self
                    .rgba_texture
                    .as_ref()
                    .is_some_and(|t| t.width() == w && t.height() == h);
                if !reuse {
                    self.rgba_texture = Some(gpu.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("mochi-rgba-frame"),
                        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    }));
                }
                let tex = self.rgba_texture.as_ref()?;
                write_rgba(gpu, tex, frame.data(), w, h);
                tex.create_view(&Default::default())
            }
            PixelFormat::Nv12 => {
                #[cfg(windows)]
                if let (Some(y), Some(uv)) = (frame.d3d11_y_handle(), frame.d3d11_uv_handle())
                    && let Ok(view) = self
                        .converter
                        .convert_nv12_from_d3d11_shared_handles(&gpu.device, &gpu.queue, y, uv, w, h)
                {
                {
                    let view = view.clone();
                    return Some(GpuFrame { view, width: w, height: h, uv_max: self.uv_max(w, h) });
                }
                }
                let (y, uv) = (frame.y_plane()?, frame.uv_plane()?);
                self.converter
                    .convert_nv12(&gpu.device, &gpu.queue, y, uv, w, h, frame.y_stride(), frame.uv_stride())
                    .map_err(|e| tracing::warn!("convert NV12: {e:?}"))
                    .ok()?
                    .clone()
            }
            PixelFormat::Yuv420p => {
                let (y, u, v) = (frame.y_plane()?, frame.u_plane()?, frame.v_plane()?);
                self.converter
                    .convert_yuv420p(&gpu.device, &gpu.queue, y, u, v, w, h, frame.y_stride(), frame.uv_stride())
                    .map_err(|e| tracing::warn!("convert YUV420p: {e:?}"))
                    .ok()?
                    .clone()
            }
        };
        let uv_max = match frame.format() {
            PixelFormat::Rgba => [1.0, 1.0],
            _ => self.uv_max(w, h),
        };
        Some(GpuFrame { view, width: w, height: h, uv_max })
    }

    fn uv_max(&self, w: u32, h: u32) -> [f32; 2] {
        match self.converter.output_texture() {
            Some(t) => [w as f32 / t.width().max(1) as f32, h as f32 / t.height().max(1) as f32],
            None => [1.0, 1.0],
        }
    }
}

/// Bản quay nhiều segment (layout Cap) phát như một video liền mạch.
/// Vòng 3 thay bằng renderer của Cap (nền, zoom, con trỏ).
pub struct RecordingSource {
    segments: Vec<SegmentSource>,
}

struct SegmentSource {
    duration: Ticks,
    decoder: AsyncVideoDecoderHandle,
    source: VideoSource,
}

impl RecordingSource {
    pub async fn open(gpu: &GpuContext, item: &MediaItem, dir: &Path) -> anyhow::Result<Self> {
        let meta = cap_project::RecordingMeta::load_for_project(dir)
            .map_err(|e| anyhow::anyhow!("recording-meta.json: {e}"))?;
        let studio = meta
            .studio_meta()
            .ok_or_else(|| anyhow::anyhow!("không phải bản quay studio"))?;
        let displays: Vec<(PathBuf, u32)> = match studio {
            cap_project::StudioRecordingMeta::SingleSegment { segment } => {
                vec![(meta.path(&segment.display.path), segment.display.fps)]
            }
            cap_project::StudioRecordingMeta::MultipleSegments { inner } => inner
                .segments
                .iter()
                .map(|s| (meta.path(&s.display.path), s.display.fps))
                .collect(),
        };
        let durations = item
            .recording
            .as_ref()
            .map(|r| r.segments.clone())
            .unwrap_or_default();
        let mut segments = Vec::new();
        for (i, (path, fps)) in displays.into_iter().enumerate() {
            let source = VideoSource::open(gpu, path, fps.max(1)).await?;
            segments.push(SegmentSource {
                duration: durations.get(i).copied().unwrap_or(Ticks::ZERO),
                decoder: source.decoder.clone(),
                source,
            });
        }
        Ok(Self { segments })
    }

    async fn frame(&mut self, gpu: &GpuContext, t: Ticks, exact: bool) -> Option<GpuFrame> {
        let mut rest = t;
        let last = self.segments.len().checked_sub(1)?;
        for (i, seg) in self.segments.iter_mut().enumerate() {
            if rest < seg.duration || i == last {
                return seg.source.frame(gpu, rest.secs(), exact).await;
            }
            rest = rest - seg.duration;
        }
        None
    }
}

/// GIF có nhiều khung → danh sách (khung, thời gian hiển thị). Ảnh khác / GIF một khung → None.
fn load_animation(gpu: &GpuContext, path: &Path) -> Option<Vec<(GpuFrame, f64)>> {
    let is_gif = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gif"));
    if !is_gif {
        return None;
    }
    let frames = mochi_project::media::gif_frames(path, 0).ok()?;
    if frames.len() < 2 {
        return None;
    }
    Some(
        frames
            .into_iter()
            .filter_map(|(img, d)| {
                let img = img?;
                let (w, h) = img.dimensions();
                let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("mochi-gif-frame"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                write_rgba(gpu, &tex, img.as_raw(), w, h);
                Some((GpuFrame::whole(tex.create_view(&Default::default()), w, h), d))
            })
            .collect(),
    )
}

fn load_image(gpu: &GpuContext, path: &Path) -> anyhow::Result<GpuFrame> {
    let img = image::open(path)?.to_rgba8();
    let (w, h) = img.dimensions();
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mochi-image"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_rgba(gpu, &tex, img.as_raw(), w, h);
    Ok(GpuFrame::whole(tex.create_view(&Default::default()), w, h))
}

fn write_rgba(gpu: &GpuContext, tex: &wgpu::Texture, data: &[u8], w: u32, h: u32) {
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// Độ dời (giây) của edit list trong luồng hình: start_time khi bỏ qua edit list − start_time thật.
pub fn editlist_shift(path: &Path) -> f64 {
    let start = |ignore: bool| -> Option<f64> {
        let mut opts = ffmpeg::Dictionary::new();
        if ignore {
            opts.set("ignore_editlist", "1");
        }
        let ctx = ffmpeg::format::input_with_dictionary(path, opts).ok()?;
        let st = ctx.streams().best(ffmpeg::media::Type::Video)?;
        let (tb, t) = (st.time_base(), st.start_time());
        (t != ffmpeg::ffi::AV_NOPTS_VALUE).then(|| t as f64 * tb.numerator() as f64 / tb.denominator().max(1) as f64)
    };
    match (start(true), start(false)) {
        (Some(raw), Some(real)) if raw - real > 0.0005 && raw - real < 2.0 => raw - real,
        _ => 0.0,
    }
}
