//! mochi-render — `Renderer::render(project, t)` ghép mọi track tại thời điểm `t` thành MỘT texture.
//! Preview và export dùng chung hàm này (S06: preview = export). Kết quả chỉ phụ thuộc
//! (project, t), không phụ thuộc đồng hồ thật.
//!
//! v1 (Phase 6): mỗi lớp là một quad (`compose::Quad`) — vị trí / scale / xoay theo keyframe (E12),
//! chuyển cảnh vào / ra (E12), màu + viền + bo góc + bóng + khung thiết bị (E15), hình khối /
//! callout / spotlight (E10).

mod cap_layer;
mod compose;
pub mod fx_map;
mod gpu;
mod overlays;
mod shapes;
mod sources;

pub use gpu::{GpuCheck, GpuContext};
pub use sources::{GpuFrame, Source};

use compose::{Mode, Quad, color};
use mochi_project::{
    Clip, ClipContent, DeviceFrame, MediaItem, Project, Ticks, TrackKind, Transition, TransitionKind,
};
use overlays::TextRasterizer;
use std::{collections::HashMap, path::PathBuf};

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// E10: làm mờ / pixelate một vùng của khung ĐÃ GHÉP bên dưới (đọc từ bản sao `scratch`).
const BLUR_SHADER: &str = r#"
struct U { rect: vec4<f32>, uv_rect: vec4<f32>, params: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let c = corners[i];
    var o: VsOut;
    o.pos = vec4<f32>(mix(u.rect.x, u.rect.z, c.x), mix(u.rect.y, u.rect.w, c.y), 0.0, 1.0);
    // uv trong canvas: (u0, v0) góc trên-trái → (u1, v1) góc dưới-phải.
    o.uv = vec2<f32>(mix(u.uv_rect.x, u.uv_rect.z, c.x), mix(u.uv_rect.w, u.uv_rect.y, c.y));
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let pixelate = u.params.x;
    let strength = u.params.y;
    let texel = u.params.zw;
    if (pixelate > 0.5) {
        let block = mix(6.0, 48.0, strength) * texel;
        let snapped = (floor(in.uv / block) + vec2<f32>(0.5)) * block;
        return vec4<f32>(textureSample(tex, samp, snapped).rgb, 1.0);
    }
    let radius = mix(4.0, 40.0, strength);
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var y = -3; y <= 3; y = y + 1) {
        for (var x = -3; x <= 3; x = x + 1) {
            let off = vec2<f32>(f32(x), f32(y)) / 3.0;
            let w = exp(-dot(off, off) * 2.0);
            acc = acc + textureSample(tex, samp, in.uv + off * radius * texel).rgb * w;
            wsum = wsum + w;
        }
    }
    return vec4<f32>(acc / wsum, 1.0);
}
"#;

/// Một lớp đã sẵn sàng để vẽ, theo thứ tự từ dưới lên.
enum Layer {
    /// Quad (ảnh nếu có `frame`, không thì vẽ theo chế độ của quad).
    Quad { frame: Option<GpuFrame>, q: Quad },
    /// `rect` theo NDC, `uv` = (x0, y0, x1, y1) chuẩn hóa 0–1 trên canvas (y hướng xuống).
    Blur { rect: [f32; 4], uv: [f32; 4], pixelate: bool, strength: f32 },
}

/// Hiệu ứng chuyển cảnh tính ra cho một clip tại một thời điểm.
#[derive(Debug, Clone, Copy)]
struct Motion {
    opacity: f32,
    dx: f32,
    dy: f32,
    scale: f32,
    wipe: f32,
    wipe_dir: f32,
    dim: f32,
    /// Typewriter: phần chữ đã hiện (0–1).
    chars: f32,
}

impl Motion {
    const NONE: Motion = Motion { opacity: 1.0, dx: 0.0, dy: 0.0, scale: 1.0, wipe: 1.0, wipe_dir: 0.0, dim: 0.0, chars: 1.0 };
}

fn ease(p: f32) -> f32 {
    mochi_project::Easing::EaseInOut.apply(p as f64) as f32
}

/// Clip đang VÀO: `p` = 0 → 1. `with_prev` = có clip trước đó trên cùng track (chuyển giữa hai clip).
fn motion_in(kind: TransitionKind, p: f32, with_prev: bool, cw: f32, ch: f32) -> Motion {
    let e = ease(p);
    let mut m = Motion::NONE;
    match kind {
        TransitionKind::Fade => m.opacity = p,
        TransitionKind::FadeBlack => {
            if with_prev && p < 0.5 {
                m.opacity = 0.0;
            } else {
                m.dim = if with_prev { 2.0 * (1.0 - p) } else { 1.0 - p };
            }
        }
        TransitionKind::SlideLeft => m.dx = (1.0 - e) * cw,
        TransitionKind::SlideRight => m.dx = -(1.0 - e) * cw,
        TransitionKind::SlideUp => m.dy = (1.0 - e) * ch,
        TransitionKind::SlideDown => m.dy = -(1.0 - e) * ch,
        TransitionKind::WipeLeft => (m.wipe, m.wipe_dir) = (p, 1.0),
        TransitionKind::WipeRight => (m.wipe, m.wipe_dir) = (p, 2.0),
        TransitionKind::Zoom => (m.scale, m.opacity) = (0.5 + 0.5 * e, p),
        TransitionKind::Typewriter => m.chars = p,
    }
    m
}

/// Clip trước đó đang RA nhường chỗ cho clip sau (`p` = tiến độ của clip sau).
fn motion_out_to_next(kind: TransitionKind, p: f32, cw: f32, ch: f32) -> Motion {
    let e = ease(p);
    let mut m = Motion::NONE;
    match kind {
        TransitionKind::Fade | TransitionKind::WipeLeft | TransitionKind::WipeRight => {}
        TransitionKind::FadeBlack => {
            if p < 0.5 {
                m.dim = 2.0 * p;
            } else {
                m.opacity = 0.0;
            }
        }
        TransitionKind::SlideLeft => m.dx = -e * cw,
        TransitionKind::SlideRight => m.dx = e * cw,
        TransitionKind::SlideUp => m.dy = -e * ch,
        TransitionKind::SlideDown => m.dy = e * ch,
        TransitionKind::Zoom => (m.scale, m.opacity) = (1.0 + 0.3 * e, 1.0 - p),
        TransitionKind::Typewriter => m.opacity = 1.0 - p,
    }
    m
}

/// Clip đang RA (không có clip sau): `q` = phần còn lại 1 → 0.
fn motion_out(kind: TransitionKind, q: f32, cw: f32, ch: f32) -> Motion {
    let e = ease(1.0 - q);
    let mut m = Motion::NONE;
    match kind {
        TransitionKind::Fade => m.opacity = q,
        TransitionKind::FadeBlack => m.dim = 1.0 - q,
        TransitionKind::SlideLeft => m.dx = -e * cw,
        TransitionKind::SlideRight => m.dx = e * cw,
        TransitionKind::SlideUp => m.dy = -e * ch,
        TransitionKind::SlideDown => m.dy = e * ch,
        TransitionKind::WipeLeft => (m.wipe, m.wipe_dir) = (q, 2.0),
        TransitionKind::WipeRight => (m.wipe, m.wipe_dir) = (q, 1.0),
        TransitionKind::Zoom => (m.scale, m.opacity) = (1.0 - 0.5 * e, q),
        TransitionKind::Typewriter => m.chars = q,
    }
    m
}

fn transition_len(tr: &Transition, clip: &Clip) -> Ticks {
    Ticks(tr.duration.0.clamp(1, clip.duration().0.max(1)))
}

pub struct Renderer {
    gpu: GpuContext,
    pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Texture 1×1 cho các quad không có ảnh.
    blank: wgpu::TextureView,
    output: Option<wgpu::Texture>,
    /// Bản sao khung đang ghép, cho lớp làm mờ đọc.
    scratch: Option<wgpu::Texture>,
    /// Nguồn đã mở, theo media id. Mở lười khi clip cần lần đầu.
    sources: HashMap<String, Source>,
    /// Đường dẫn đã mở cho từng nguồn — đổi (vd. sang proxy) thì mở lại.
    source_paths: HashMap<String, PathBuf>,
    failed: HashMap<String, String>,
    text: Option<TextRasterizer>,
    /// Ảnh chữ / hình khối / callout đã vẽ (theo nội dung + độ phân giải).
    raster_cache: HashMap<String, (GpuFrame, [f32; 2])>,
}

/// Thông tin một lần render (để đo hiệu năng / hiển thị).
#[derive(Debug, Clone, Default)]
pub struct RenderStats {
    pub layers: usize,
    pub missing_frames: usize,
}

impl Renderer {
    pub fn new(gpu: GpuContext) -> Self {
        let device = &gpu.device;
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mochi-quad-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let make_pipeline = |label: &str, src: &str, blend: Option<wgpu::BlendState>| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[&bgl],
                    push_constant_ranges: &[],
                })),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: OUTPUT_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline("mochi-quad", compose::QUAD_SHADER, Some(wgpu::BlendState::ALPHA_BLENDING));
        let blur_pipeline = make_pipeline("mochi-blur", BLUR_SHADER, None);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mochi-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let blank = upload_rgba(&gpu, &[255, 255, 255, 255], 1, 1, "mochi-blank").create_view(&Default::default());
        Self {
            gpu,
            pipeline,
            blur_pipeline,
            bgl,
            sampler,
            blank,
            output: None,
            scratch: None,
            sources: HashMap::new(),
            source_paths: HashMap::new(),
            failed: HashMap::new(),
            text: None,
            raster_cache: HashMap::new(),
        }
    }

    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// Bỏ nguồn của media không còn trong project (giải phóng decoder).
    pub fn retain_media(&mut self, project: &Project) {
        self.sources
            .retain(|id, _| project.media.iter().any(|m| &m.id == id));
        self.source_paths.retain(|id, _| self.sources.contains_key(id));
        self.failed.clear();
        if self.raster_cache.len() > 96 {
            self.raster_cache.clear();
        }
    }

    /// Bỏ mọi nguồn (đóng decoder và file media).
    pub fn clear(&mut self) {
        self.sources.clear();
        self.source_paths.clear();
        self.failed.clear();
        self.raster_cache.clear();
    }

    /// Lỗi mở nguồn (media id → lý do), để UI hiển thị.
    pub fn failures(&self) -> &HashMap<String, String> {
        &self.failed
    }

    /// Ghép mọi track tại `t`. `paths`: media id → đường dẫn thật (media thiếu thì không có).
    /// `exact` = seek chính xác từng frame (tua, xuất); khi đang phát thì để false.
    pub async fn render(
        &mut self,
        project: &Project,
        paths: &HashMap<String, PathBuf>,
        t: Ticks,
        exact: bool,
    ) -> (wgpu::Texture, RenderStats) {
        self.render_sized(project, paths, t, exact, None).await
    }

    /// Như `render` nhưng ra khung `size` (xuất ở độ phân giải khác canvas, cùng tỉ lệ).
    /// Mọi thứ tính theo tỉ lệ khung; riêng cỡ chữ / nét (pixel của canvas) được nhân theo.
    pub async fn render_sized(
        &mut self,
        project: &Project,
        paths: &HashMap<String, PathBuf>,
        t: Ticks,
        exact: bool,
        size: Option<(u32, u32)>,
    ) -> (wgpu::Texture, RenderStats) {
        let (cw, ch) = size.unwrap_or((project.canvas.width, project.canvas.height));
        let (cw, ch) = (cw.max(2), ch.max(2));
        let scale = cw as f32 / project.canvas.width.max(2) as f32;
        let output = self.ensure_output(cw, ch);
        let mut stats = RenderStats::default();
        let frame_len = Ticks(mochi_project::TICKS_PER_SECOND / project.canvas.fps.max(1) as i64);

        // Track đầu danh sách ở dưới cùng.
        let mut layers = Vec::new();
        for track in &project.tracks {
            if track.hidden || track.kind == TrackKind::Audio {
                continue;
            }
            let on_track: Vec<&Clip> = project.clips.iter().filter(|c| c.track_id == track.id).collect();
            let Some(clip) = on_track.iter().copied().find(|c| c.start <= t && t < c.end()) else {
                continue;
            };
            let (fw, fh) = (cw as f32, ch as f32);
            let adjacent = |a: &Clip, b: &Clip| (a.end().0 - b.start.0).abs() <= frame_len.0;
            let mut items: Vec<(&Clip, Motion)> = Vec::new();
            let incoming = clip.transition_in.as_ref().map(|tr| (tr, transition_len(tr, clip))).filter(|(_, d)| t < clip.start + *d);
            if let Some((tr, d)) = incoming {
                let p = ((t - clip.start).0 as f64 / d.0 as f64) as f32;
                let prev = on_track.iter().copied().find(|c| c.id != clip.id && adjacent(c, clip));
                if let Some(prev) = prev {
                    items.push((prev, motion_out_to_next(tr.kind, p, fw, fh)));
                }
                items.push((clip, motion_in(tr.kind, p, prev.is_some(), fw, fh)));
            } else {
                let next_takes_over = on_track
                    .iter()
                    .any(|c| c.id != clip.id && c.transition_in.is_some() && adjacent(clip, c));
                let outgoing = clip
                    .transition_out
                    .as_ref()
                    .filter(|_| !next_takes_over)
                    .map(|tr| (tr, transition_len(tr, clip)))
                    .filter(|(_, d)| t >= clip.end() - *d);
                match outgoing {
                    Some((tr, d)) => {
                        let q = ((clip.end() - t).0 as f64 / d.0 as f64) as f32;
                        items.push((clip, motion_out(tr.kind, q.clamp(0.0, 1.0), fw, fh)));
                    }
                    None => items.push((clip, Motion::NONE)),
                }
            }
            for (c, m) in items {
                if m.opacity <= 0.001 {
                    continue;
                }
                self.clip_layers(project, c, m, paths, t, exact, (cw, ch), scale, &mut layers, &mut stats).await;
            }
        }
        stats.layers = layers.len();
        self.draw(&output, &layers);
        (output, stats)
    }

    /// Lớp của một clip tại `t` (đã áp keyframe, chuyển cảnh, màu & khung).
    #[allow(clippy::too_many_arguments)]
    async fn clip_layers(
        &mut self,
        project: &Project,
        clip: &Clip,
        m: Motion,
        paths: &HashMap<String, PathBuf>,
        t: Ticks,
        exact: bool,
        (cw, ch): (u32, u32),
        scale: f32,
        out: &mut Vec<Layer>,
        stats: &mut RenderStats,
    ) {
        let (fw, fh) = (cw as f32, ch as f32);
        // Lúc chuyển cảnh, clip trước được vẽ tiếp quá điểm kết thúc của nó.
        let tf = clip.transform_at(t.min(clip.end()));
        let opacity = (tf.opacity as f32 * m.opacity).clamp(0.0, 1.0);
        let rotation = (tf.rotation as f32).to_radians();
        match &clip.content {
            ClipContent::Text { text, font, size_px, color: c, background } => {
                let shown: String = if m.chars < 1.0 {
                    let n = text.chars().count();
                    text.chars().take((n as f32 * m.chars).floor() as usize).collect()
                } else {
                    text.clone()
                };
                if shown.is_empty() {
                    return;
                }
                let key = format!("t\u{1}{shown}\u{1}{font}\u{1}{size_px}\u{1}{c}\u{1}{background:?}\u{1}{cw}");
                let full_w = if m.chars < 1.0 {
                    let k = format!("t\u{1}{text}\u{1}{font}\u{1}{size_px}\u{1}{c}\u{1}{background:?}\u{1}{cw}");
                    self.text_raster(&k, text, font, *size_px as f32 * scale, c, background.as_deref(), cw).width as f32
                } else {
                    0.0
                };
                let frame = self.text_raster(&key, &shown, font, *size_px as f32 * scale, c, background.as_deref(), cw);
                let s = tf.scale as f32 * m.scale;
                let mut cx = tf.x as f32 * fw + m.dx;
                if m.chars < 1.0 {
                    // Gõ chữ: giữ mép trái cố định như khi hiện đủ chữ.
                    cx += (frame.width as f32 - full_w) * s / 2.0;
                }
                let mut q = Quad::new(Mode::Image, [cx, tf.y as f32 * fh + m.dy], [frame.width as f32 * s / 2.0, frame.height as f32 * s / 2.0]);
                q.rotation = rotation;
                apply_motion(&mut q, m, opacity);
                out.push(Layer::Quad { frame: Some(frame), q });
            }
            ClipContent::Blur { x, y, width, height, pixelate, strength } => {
                let (x0, y0, x1, y1) = (*x as f32, *y as f32, (*x + *width) as f32, (*y + *height) as f32);
                out.push(Layer::Blur {
                    rect: [x0 * 2.0 - 1.0, 1.0 - y1 * 2.0, x1 * 2.0 - 1.0, 1.0 - y0 * 2.0],
                    uv: [x0, y0, x1, y1],
                    pixelate: *pixelate,
                    strength: *strength as f32,
                });
            }
            ClipContent::Shape { shape, x0, y0, x1, y1, stroke, stroke_width, fill } => {
                let key = format!("s\u{1}{shape:?}{x0}{y0}{x1}{y1}{stroke}{stroke_width}{fill:?}\u{1}{cw}x{ch}");
                let Some((frame, center)) = self.raster(&key, || {
                    shapes::shape(*shape, (*x0, *y0, *x1, *y1), stroke, *stroke_width, fill.as_deref(), cw, ch, scale)
                }) else {
                    return;
                };
                let s = tf.scale as f32 * m.scale;
                let c = [center[0] + (tf.x as f32 - 0.5) * fw + m.dx, center[1] + (tf.y as f32 - 0.5) * fh + m.dy];
                let mut q = Quad::new(Mode::Image, c, [frame.width as f32 * s / 2.0, frame.height as f32 * s / 2.0]);
                q.rotation = rotation;
                apply_motion(&mut q, m, opacity);
                out.push(Layer::Quad { frame: Some(frame), q });
            }
            ClipContent::Callout { text, font, size_px, color: c, background, tail_x, tail_y } => {
                let (bx, by) = (tf.x as f32 * fw + m.dx, tf.y as f32 * fh + m.dy);
                let (tx, ty) = (*tail_x as f32 * fw + m.dx, *tail_y as f32 * fh + m.dy);
                let shown: String = if m.chars < 1.0 {
                    let n = text.chars().count();
                    text.chars().take(((n as f32 * m.chars).floor() as usize).max(1)).collect()
                } else {
                    text.clone()
                };
                let key = format!("c\u{1}{shown}{font}{size_px}{c}{background}{bx}{by}{tx}{ty}\u{1}{cw}x{ch}");
                let size = *size_px as f32 * scale;
                if !self.raster_cache.contains_key(&key) {
                    let tr = self.text.get_or_insert_with(TextRasterizer::new);
                    let drawn = shapes::callout(tr, &shown, font, size, c, background, (bx, by), (tx, ty), fw * 0.6);
                    self.raster(&key, || drawn);
                }
                let Some((frame, center)) = self.raster_cache.get(&key).cloned() else {
                    return;
                };
                let mut q = Quad::new(Mode::Image, center, [frame.width as f32 / 2.0, frame.height as f32 / 2.0]);
                apply_motion(&mut q, m, opacity);
                out.push(Layer::Quad { frame: Some(frame), q });
            }
            ClipContent::Spotlight { x, y, width, height, ellipse, dim } => {
                let mut q = Quad::new(Mode::Spotlight, [fw / 2.0, fh / 2.0], [fw / 2.0, fh / 2.0]);
                q.spot = [
                    *x as f32 * fw + m.dx,
                    *y as f32 * fh + m.dy,
                    (*x + *width) as f32 * fw + m.dx,
                    (*y + *height) as f32 * fh + m.dy,
                ];
                q.border = if *ellipse { 1.0 } else { 0.0 };
                q.radius = fw.min(fh) * 0.02;
                q.fill = [0.0, 0.0, 0.0, dim.clamp(0.0, 1.0) as f32];
                q.opacity = opacity;
                out.push(Layer::Quad { frame: None, q });
            }
            _ => {
                let Some(item) = clip.media_id.as_ref().and_then(|id| project.media.iter().find(|x| &x.id == id)) else {
                    return;
                };
                let Some(frame) = self.media_frame(item, clip, paths, t, exact, cw, ch).await else {
                    stats.missing_frames += 1;
                    return;
                };
                let fit = (fw / frame.width as f32).min(fh / frame.height as f32);
                let s = fit * tf.scale as f32 * m.scale;
                let mut half = [frame.width as f32 * s / 2.0, frame.height as f32 * s / 2.0];
                let center = [tf.x as f32 * fw + m.dx, tf.y as f32 * fh + m.dy];
                let look = &clip.look;
                let device = device_frame(look.device, center, &mut half, (fw, fh), scale);
                let short = (half[0] * 2.0).min(half[1] * 2.0);
                let radius = device.content_radius.unwrap_or(look.corner_radius as f32 / 100.0 * short);
                // Đổ bóng dưới khung thiết bị (nếu có) hoặc dưới clip.
                if look.shadow > 0.0 {
                    let (sc, sh, sr) = device.outline.unwrap_or((center, half, radius));
                    let mut q = Quad::new(Mode::Shadow, sc, sh);
                    q.radius = sr;
                    q.extent = (short * 0.06).max(6.0 * scale) * (0.5 + look.shadow as f32 / 100.0);
                    q.fill = [0.0, 0.0, 0.0, 0.65 * look.shadow as f32 / 100.0];
                    q.rotation = rotation;
                    apply_motion(&mut q, m, opacity);
                    q.wipe_dir = 0.0;
                    out.push(Layer::Quad { frame: None, q });
                }
                for mut q in device.under {
                    q.rotation = rotation;
                    apply_motion(&mut q, m, opacity);
                    out.push(Layer::Quad { frame: None, q });
                }
                let mut q = Quad::new(Mode::Image, center, half);
                q.uv_max = frame.uv_max;
                q.rotation = rotation;
                q.radius = radius;
                q.border = look.border_width as f32 * scale;
                q.border_color = color(&look.border_color);
                q.brightness = look.brightness as f32 / 100.0;
                q.contrast = look.contrast as f32 / 100.0;
                q.saturation = look.saturation as f32 / 100.0;
                apply_motion(&mut q, m, opacity);
                out.push(Layer::Quad { frame: Some(frame), q });
                for mut q in device.over {
                    q.rotation = rotation;
                    apply_motion(&mut q, m, opacity);
                    out.push(Layer::Quad { frame: None, q });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text_raster(&mut self, key: &str, text: &str, font: &str, size: f32, c: &str, bg: Option<&str>, cw: u32) -> GpuFrame {
        if let Some((f, _)) = self.raster_cache.get(key) {
            return f.clone();
        }
        let raster = self.text.get_or_insert_with(TextRasterizer::new);
        let (w, h, rgba) = raster.rasterize(text, font, size, c, bg, cw as f32 * 0.9);
        let frame = GpuFrame::whole(upload_rgba(&self.gpu, &rgba, w, h, "mochi-text").create_view(&Default::default()), w, h);
        self.raster_cache.insert(key.to_string(), (frame.clone(), [0.0, 0.0]));
        frame
    }

    fn raster(&mut self, key: &str, draw: impl FnOnce() -> Option<shapes::Raster>) -> Option<(GpuFrame, [f32; 2])> {
        if let Some(v) = self.raster_cache.get(key) {
            return Some(v.clone());
        }
        let r = draw()?;
        let tex = upload_rgba(&self.gpu, &r.rgba, r.width, r.height, "mochi-shape");
        let v = (GpuFrame::whole(tex.create_view(&Default::default()), r.width, r.height), r.center);
        self.raster_cache.insert(key.to_string(), v.clone());
        Some(v)
    }

    #[allow(clippy::too_many_arguments)]
    async fn media_frame(
        &mut self,
        item: &MediaItem,
        clip: &Clip,
        paths: &HashMap<String, PathBuf>,
        t: Ticks,
        exact: bool,
        cw: u32,
        ch: u32,
    ) -> Option<GpuFrame> {
        if self.failed.contains_key(&item.id) {
            return None;
        }
        let path = paths.get(&item.id)?;
        if self.source_paths.get(&item.id).is_some_and(|p| p != path) {
            self.sources.remove(&item.id);
        }
        if !self.sources.contains_key(&item.id) {
            self.source_paths.insert(item.id.clone(), path.clone());
            match Source::open(&self.gpu, item, path).await {
                Ok(s) => {
                    self.sources.insert(item.id.clone(), s);
                }
                Err(e) => {
                    tracing::warn!("mở nguồn {} lỗi: {e:#}", item.name);
                    self.failed.insert(item.id.clone(), format!("{e:#}"));
                    return None;
                }
            }
        }
        let source = self.sources.get_mut(&item.id)?;
        // Ảnh tĩnh bỏ qua thời gian; GIF động (E01) cần thời gian trong clip để phát. Lúc chuyển
        // cảnh, clip trước có thể chạy quá điểm out → kẹp trong độ dài media.
        let mut src_t = clip.source_time_at(t);
        if item.duration.0 > 0 {
            src_t = Ticks(src_t.0.clamp(0, (item.duration.0 - mochi_project::TICKS_PER_SECOND / 60).max(0)));
        }
        let fx = match &clip.content {
            ClipContent::Recording { fx } => Some(fx),
            _ => None,
        };
        source.frame(&self.gpu, src_t, exact, fx, (cw, ch)).await
    }

    fn ensure_output(&mut self, w: u32, h: u32) -> wgpu::Texture {
        let make = |label: &str, extra: wgpu::TextureUsages| {
            self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OUTPUT_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | extra,
                view_formats: &[],
            })
        };
        let reuse = self.output.as_ref().is_some_and(|t| t.width() == w && t.height() == h);
        if !reuse {
            self.output = Some(make(
                "mochi-canvas",
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            ));
            self.scratch = Some(make("mochi-scratch", wgpu::TextureUsages::COPY_DST));
        }
        self.output.clone().unwrap()
    }

    fn bind(&self, uniform: &[f32], view: &wgpu::TextureView) -> wgpu::BindGroup {
        let device = &self.gpu.device;
        let bytes: Vec<u8> = uniform.iter().flat_map(|v| v.to_le_bytes()).collect();
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mochi-u"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.gpu.queue.write_buffer(&buf, 0, &bytes);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// Vẽ từ dưới lên. Gặp lớp làm mờ thì kết thúc pass, chép khung hiện tại sang `scratch`,
    /// rồi vẽ vùng đó lại bằng shader blur/pixelate đọc từ `scratch`.
    fn draw(&self, output: &wgpu::Texture, layers: &[Layer]) {
        let device = &self.gpu.device;
        let view = output.create_view(&Default::default());
        let scratch = self.scratch.as_ref().expect("scratch theo output");
        let scratch_view = scratch.create_view(&Default::default());
        let (w, h) = (output.width(), output.height());
        let mut enc = device.create_command_encoder(&Default::default());
        let mut first = true;
        let mut i = 0;
        while i < layers.len() || first {
            // Gom các quad liên tiếp vào một pass.
            let mut groups = Vec::new();
            while i < layers.len() {
                match &layers[i] {
                    Layer::Quad { frame, q } => {
                        let u = q.uniform((w, h));
                        groups.push(self.bind(&u, frame.as_ref().map_or(&self.blank, |f| &f.view)));
                        i += 1;
                    }
                    Layer::Blur { .. } => break,
                }
            }
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mochi-compose"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if first { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.pipeline);
                for g in &groups {
                    pass.set_bind_group(0, g, &[]);
                    pass.draw(0..6, 0..1);
                }
            }
            first = false;
            // Lớp làm mờ: chép khung → scratch, vẽ lại vùng.
            if let Some(Layer::Blur { rect, uv, pixelate, strength }) = layers.get(i) {
                enc.copy_texture_to_texture(
                    output.as_image_copy(),
                    scratch.as_image_copy(),
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
                let u = [
                    rect[0], rect[1], rect[2], rect[3],
                    uv[0], uv[1], uv[2], uv[3],
                    if *pixelate { 1.0 } else { 0.0 }, *strength, 1.0 / w as f32, 1.0 / h as f32,
                ];
                let g = self.bind(&u, &scratch_view);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mochi-blur"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.blur_pipeline);
                pass.set_bind_group(0, &g, &[]);
                pass.draw(0..6, 0..1);
                i += 1;
            }
        }
        self.gpu.queue.submit([enc.finish()]);
    }
}

fn apply_motion(q: &mut Quad, m: Motion, opacity: f32) {
    q.opacity = opacity;
    q.wipe = m.wipe;
    q.wipe_dir = m.wipe_dir;
    q.dim = m.dim.clamp(0.0, 1.0);
}

/// Khung thiết bị quanh clip (E15): các quad vẽ dưới / trên clip, bo góc của clip, đường bao để đổ bóng.
struct Device {
    under: Vec<Quad>,
    over: Vec<Quad>,
    content_radius: Option<f32>,
    outline: Option<([f32; 2], [f32; 2], f32)>,
}

fn solid(center: [f32; 2], half: [f32; 2], radius: f32, fill: &str) -> Quad {
    let mut q = Quad::new(Mode::Solid, center, half);
    q.radius = radius;
    q.fill = color(fill);
    q
}

/// Dựng khung thiết bị; thu nhỏ `half` của clip nếu cả khung tràn khỏi canvas.
fn device_frame(kind: DeviceFrame, c: [f32; 2], half: &mut [f32; 2], (fw, fh): (f32, f32), scale: f32) -> Device {
    let none = Device { under: vec![], over: vec![], content_radius: None, outline: None };
    if kind == DeviceFrame::None {
        return none;
    }
    // Kích thước ngoài của khung (theo nửa rộng / nửa cao của clip) → thu nhỏ cho vừa canvas.
    let extent = |h: [f32; 2]| -> [f32; 2] {
        match kind {
            DeviceFrame::Laptop => {
                let m = 0.04 * 2.0 * h[0];
                [h[0] * 1.16 + m * 1.16, h[1] + m * 1.0 + m * 2.2]
            }
            DeviceFrame::Phone => {
                let m = 0.05 * 2.0 * h[0].min(h[1]);
                [h[0] + m, h[1] + m]
            }
            DeviceFrame::Browser => {
                let bar = (0.075 * 2.0 * h[1]).max(18.0 * scale);
                [h[0] + 1.0, h[1] + bar / 2.0 + 1.0]
            }
            DeviceFrame::None => h,
        }
    };
    let e = extent(*half);
    let k = (fw * 0.48 / e[0]).min(fh * 0.48 / e[1]).min(1.0);
    half[0] *= k;
    half[1] *= k;
    let h = *half;
    match kind {
        DeviceFrame::Laptop => {
            let m = 0.04 * 2.0 * h[0];
            let bezel_c = [c[0], c[1] + m * 0.075];
            let bezel_h = [h[0] + m, h[1] + m * 1.075];
            let base_hh = m * 0.45;
            let base_c = [c[0], bezel_c[1] + bezel_h[1] + base_hh];
            let base_h = [bezel_h[0] * 1.16, base_hh];
            Device {
                under: vec![
                    solid(bezel_c, bezel_h, m * 0.9, "#1B1B1F"),
                    solid(base_c, base_h, base_hh, "#BFC3C9"),
                    solid([c[0], base_c[1] - base_hh * 0.55], [h[0] * 0.13, base_hh * 0.35], base_hh * 0.35, "#9DA2A9"),
                    solid([c[0], c[1] - h[1] - m * 0.5], [m * 0.16, m * 0.16], m * 0.16, "#3A3A40"),
                ],
                over: vec![],
                content_radius: Some(m * 0.15),
                outline: Some((bezel_c, bezel_h, m * 0.9)),
            }
        }
        DeviceFrame::Phone => {
            let m = 0.05 * 2.0 * h[0].min(h[1]);
            let bezel_h = [h[0] + m, h[1] + m];
            let r = bezel_h[0].min(bezel_h[1]) * 0.22;
            let mut bezel = solid(c, bezel_h, r, "#111114");
            bezel.border = (2.0 * scale).max(1.0);
            bezel.border_color = color("#3C3C44");
            let notch_h = [h[0].min(h[1]) * 0.16, m * 0.45];
            Device {
                under: vec![bezel],
                over: vec![solid([c[0], c[1] - h[1] + notch_h[1] + m * 0.35], notch_h, notch_h[1], "#000000")],
                content_radius: Some((r - m).max(r * 0.5)),
                outline: Some((c, bezel_h, r)),
            }
        }
        DeviceFrame::Browser => {
            let bar = (0.075 * 2.0 * h[1]).max(18.0 * scale);
            let r = 10.0 * scale;
            let win_c = [c[0], c[1] - bar / 2.0];
            let win_h = [h[0] + 1.0, h[1] + bar / 2.0 + 1.0];
            let mut win = solid(win_c, win_h, r, "#E8EAED");
            win.border = 1.0;
            win.border_color = color("#C8CACE");
            let y = c[1] - h[1] - bar / 2.0;
            let dot = bar * 0.15;
            let left = c[0] - h[0];
            let mut under = vec![win];
            for (i, col) in ["#FF5F57", "#FEBC2E", "#28C840"].iter().enumerate() {
                under.push(solid([left + bar * 0.5 + i as f32 * bar * 0.42, y], [dot, dot], dot, col));
            }
            under.push(solid([c[0] + bar * 0.4, y], [h[0] * 0.55, bar * 0.27], bar * 0.27, "#FFFFFF"));
            Device { under, over: vec![], content_radius: Some(r * 0.4), outline: Some((win_c, win_h, r)) }
        }
        DeviceFrame::None => none,
    }
}

fn upload_rgba(gpu: &GpuContext, rgba: &[u8], w: u32, h: u32, label: &str) -> wgpu::Texture {
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    tex
}

/// Đặt ảnh (w×h, đơn vị pixel canvas) tại tâm (x, y theo tỉ lệ canvas), nhân `scale`. NDC.
pub fn pixel_rect(w: u32, h: u32, cw: u32, ch: u32, x: f64, y: f64, scale: f64) -> [f32; 4] {
    let (pw, ph) = (w as f64 * scale, h as f64 * scale);
    let (cx, cy) = (x * cw as f64, y * ch as f64);
    let to_ndc_x = |px: f64| (px / cw as f64 * 2.0 - 1.0) as f32;
    let to_ndc_y = |py: f64| (1.0 - py / ch as f64 * 2.0) as f32;
    [to_ndc_x(cx - pw / 2.0), to_ndc_y(cy + ph / 2.0), to_ndc_x(cx + pw / 2.0), to_ndc_y(cy - ph / 2.0)]
}

/// Đặt khung hình (w×h) vào canvas (cw×ch) kiểu "contain", rồi áp transform
/// (tâm x, y theo tỉ lệ canvas, scale). Trả về (x0, y0, x1, y1) theo NDC (y hướng lên).
pub fn fit_rect(w: u32, h: u32, cw: u32, ch: u32, x: f64, y: f64, scale: f64) -> [f32; 4] {
    let s = (cw as f64 / w as f64).min(ch as f64 / h as f64) * scale;
    let (pw, ph) = (w as f64 * s, h as f64 * s);
    let (cx, cy) = (x * cw as f64, y * ch as f64);
    let to_ndc_x = |px: f64| (px / cw as f64 * 2.0 - 1.0) as f32;
    let to_ndc_y = |py: f64| (1.0 - py / ch as f64 * 2.0) as f32;
    [
        to_ndc_x(cx - pw / 2.0),
        to_ndc_y(cy + ph / 2.0),
        to_ndc_x(cx + pw / 2.0),
        to_ndc_y(cy - ph / 2.0),
    ]
}

/// Đọc texture RGBA8 về CPU (dùng cho kiểm thử, ảnh thu nhỏ, xuất GIF/PNG).
pub fn read_texture_rgba(gpu: &GpuContext, tex: &wgpu::Texture) -> Vec<u8> {
    let (w, h) = (tex.width(), tex.height());
    let padded = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mochi-readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::Wait);
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h {
        let start = (row * padded) as usize;
        out.extend_from_slice(&data[start..start + (w * 4) as usize]);
    }
    out
}
