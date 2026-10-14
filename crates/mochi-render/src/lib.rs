//! mochi-render — `Renderer::render(project, t)` ghép mọi track tại thời điểm `t` thành MỘT texture.
//! Preview và export dùng chung hàm này (S06: preview = export). Kết quả chỉ phụ thuộc
//! (project, t), không phụ thuộc đồng hồ thật.

mod cap_layer;
pub mod fx_map;
mod gpu;
mod overlays;
mod sources;

pub use gpu::{GpuCheck, GpuContext};
pub use sources::{GpuFrame, Source};

use mochi_project::{Clip, ClipContent, MediaItem, Project, Ticks, TrackKind};
use overlays::TextRasterizer;
use std::{collections::HashMap, path::PathBuf};

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const QUAD_SHADER: &str = r#"
struct U { rect: vec4<f32>, opacity: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // 2 tam giác phủ hình chữ nhật u.rect = (x0, y0, x1, y1) theo NDC.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let c = corners[i];
    var o: VsOut;
    o.pos = vec4<f32>(mix(u.rect.x, u.rect.z, c.x), mix(u.rect.y, u.rect.w, c.y), 0.0, 1.0);
    // opacity.yz = phần texture chứa hình (uv_max).
    o.uv = vec2<f32>(c.x, 1.0 - c.y) * u.opacity.yz;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Không lấy mẫu lấn sang phần texture ngoài hình (lọc song tuyến ở mép phải / dưới).
    let half_texel = 0.5 / vec2<f32>(textureDimensions(tex));
    let c = textureSample(tex, samp, min(in.uv, u.opacity.yz - half_texel));
    return vec4<f32>(c.rgb, c.a * u.opacity.x);
}
"#;

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
    Picture { frame: GpuFrame, rect: [f32; 4], opacity: f32 },
    /// `rect` theo NDC, `uv` = (x0, y0, x1, y1) chuẩn hóa 0–1 trên canvas (y hướng xuống).
    Blur { rect: [f32; 4], uv: [f32; 4], pixelate: bool, strength: f32 },
}

pub struct Renderer {
    gpu: GpuContext,
    pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    output: Option<wgpu::Texture>,
    /// Bản sao khung đang ghép, cho lớp làm mờ đọc.
    scratch: Option<wgpu::Texture>,
    /// Nguồn đã mở, theo media id. Mở lười khi clip cần lần đầu.
    sources: HashMap<String, Source>,
    /// Đường dẫn đã mở cho từng nguồn — đổi (vd. sang proxy) thì mở lại.
    source_paths: HashMap<String, PathBuf>,
    failed: HashMap<String, String>,
    text: Option<TextRasterizer>,
    text_cache: HashMap<String, GpuFrame>,
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
        let pipeline = make_pipeline("mochi-quad", QUAD_SHADER, Some(wgpu::BlendState::ALPHA_BLENDING));
        let blur_pipeline = make_pipeline("mochi-blur", BLUR_SHADER, None);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mochi-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            gpu,
            pipeline,
            blur_pipeline,
            bgl,
            sampler,
            output: None,
            scratch: None,
            sources: HashMap::new(),
            source_paths: HashMap::new(),
            failed: HashMap::new(),
            text: None,
            text_cache: HashMap::new(),
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
        if self.text_cache.len() > 64 {
            self.text_cache.clear();
        }
    }

    /// Bỏ mọi nguồn (đóng decoder và file media).
    pub fn clear(&mut self) {
        self.sources.clear();
        self.source_paths.clear();
        self.failed.clear();
        self.text_cache.clear();
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
    /// Mọi thứ tính theo tỉ lệ khung; riêng cỡ chữ (pixel của canvas) được nhân theo.
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
        let text_scale = cw as f32 / project.canvas.width.max(2) as f32;
        let output = self.ensure_output(cw, ch);
        let mut stats = RenderStats::default();

        // Track đầu danh sách ở dưới cùng.
        let mut layers = Vec::new();
        for track in &project.tracks {
            if track.hidden || track.kind == TrackKind::Audio {
                continue;
            }
            let Some(clip) = project
                .clips
                .iter()
                .find(|c| c.track_id == track.id && c.start <= t && t < c.end())
            else {
                continue;
            };
            match &clip.content {
                ClipContent::Text { text, font, size_px, color, background } => {
                    let frame = self.text_frame(text, font, *size_px as f32 * text_scale, color, background.as_deref(), cw);
                    let tf = clip.transform.unwrap_or_default();
                    layers.push(Layer::Picture {
                        rect: pixel_rect(frame.width, frame.height, cw, ch, tf.x, tf.y, tf.scale),
                        opacity: tf.opacity as f32,
                        frame,
                    });
                }
                ClipContent::Blur { x, y, width, height, pixelate, strength } => {
                    let (x0, y0, x1, y1) = (*x as f32, *y as f32, (*x + *width) as f32, (*y + *height) as f32);
                    layers.push(Layer::Blur {
                        rect: [x0 * 2.0 - 1.0, 1.0 - y1 * 2.0, x1 * 2.0 - 1.0, 1.0 - y0 * 2.0],
                        uv: [x0, y0, x1, y1],
                        pixelate: *pixelate,
                        strength: *strength as f32,
                    });
                }
                _ => {
                    let Some(item) = clip
                        .media_id
                        .as_ref()
                        .and_then(|id| project.media.iter().find(|m| &m.id == id))
                    else {
                        continue;
                    };
                    match self.layer_for(item, clip, paths, t, exact, cw, ch).await {
                        Some(l) => layers.push(l),
                        None => stats.missing_frames += 1,
                    }
                }
            }
        }
        stats.layers = layers.len();
        self.draw(&output, &layers);
        (output, stats)
    }

    fn text_frame(&mut self, text: &str, font: &str, size: f32, color: &str, bg: Option<&str>, cw: u32) -> GpuFrame {
        let key = format!("{text}\u{1}{font}\u{1}{size}\u{1}{color}\u{1}{bg:?}\u{1}{cw}");
        if let Some(f) = self.text_cache.get(&key) {
            return f.clone();
        }
        let raster = self.text.get_or_insert_with(TextRasterizer::new);
        let (w, h, rgba) = raster.rasterize(text, font, size, color, bg, cw as f32 * 0.9);
        let tex = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mochi-text"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let frame = GpuFrame::whole(tex.create_view(&Default::default()), w, h);
        self.text_cache.insert(key, frame.clone());
        frame
    }

    #[allow(clippy::too_many_arguments)]
    async fn layer_for(
        &mut self,
        item: &MediaItem,
        clip: &Clip,
        paths: &HashMap<String, PathBuf>,
        t: Ticks,
        exact: bool,
        cw: u32,
        ch: u32,
    ) -> Option<Layer> {
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
        // Ảnh tĩnh bỏ qua thời gian; GIF động (E01) cần thời gian trong clip để phát.
        let src_t = clip.source_time_at(t);
        let fx = match &clip.content {
            ClipContent::Recording { fx } => Some(fx),
            _ => None,
        };
        let frame = source.frame(&self.gpu, src_t, exact, fx, (cw, ch)).await?;
        let tf = clip.transform.unwrap_or_default();
        Some(Layer::Picture {
            rect: fit_rect(frame.width, frame.height, cw, ch, tf.x, tf.y, tf.scale),
            opacity: tf.opacity as f32,
            frame,
        })
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
            // Gom các lớp hình liên tiếp vào một pass.
            let mut groups = Vec::new();
            while i < layers.len() {
                match &layers[i] {
                    Layer::Picture { frame, rect, opacity } => {
                        let u = [rect[0], rect[1], rect[2], rect[3], *opacity, frame.uv_max[0], frame.uv_max[1], 0.0];
                        groups.push((false, self.bind(&u, &frame.view)));
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
                for (_, g) in &groups {
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
