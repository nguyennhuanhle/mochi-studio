//! Clip bản quay vẽ bằng renderer của Cap (nền, khung, zoom lò xo, con trỏ, webcam) trên CHÍNH
//! wgpu device của Mochi, rồi đưa texture kết quả vào compositor nhiều track.
//! Cần bản vá `[MOCHI] render_frame_to_texture` trong vendor/cap (xem PROVENANCE.md).

use crate::{GpuContext, GpuFrame, fx_map};
use cap_editor::SegmentMedia;
use cap_project::{ProjectConfiguration, StudioRecordingMeta, XY};
use cap_rendering::{
    CapRenderSession, PrecomputedCursorTimeline, ProjectRecordingsMeta, ProjectUniforms,
    RenderVideoConstants, RendererLayers, SharedWgpuDevice, ZoomTransformTimeline,
    spring_mass_damper::SpringMassDamperSimulationConfig,
};
use mochi_project::{MediaItem, RecordingFx, Ticks};
use std::path::Path;

pub struct CapRecordingLayer {
    constants: RenderVideoConstants,
    layers: RendererLayers,
    session: CapRenderSession,
    segments: Vec<SegmentMedia>,
    segment_secs: Vec<f64>,
    has_camera: bool,
    /// Cấu hình Cap hiện tại + khóa (fx, canvas) để biết khi nào phải dựng lại timeline zoom/cursor.
    config: ProjectConfiguration,
    key: Option<(RecordingFx, (u32, u32))>,
    zoom: Vec<ZoomTransformTimeline>,
    cursor: Vec<PrecomputedCursorTimeline>,
    fps: u32,
}

impl CapRecordingLayer {
    pub async fn open(gpu: &GpuContext, item: &MediaItem, dir: &Path) -> anyhow::Result<Self> {
        let recording_meta = cap_project::RecordingMeta::load_for_project(dir)
            .map_err(|e| anyhow::anyhow!("recording-meta.json: {e}"))?;
        let studio: StudioRecordingMeta = recording_meta
            .studio_meta()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("không phải bản quay studio"))?;
        let segments = cap_editor::create_segments_without_audio(&recording_meta, &studio, false)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let render_segments = ProjectRecordingsMeta::new(&recording_meta.project_path, &studio)
            .map_err(|e| anyhow::anyhow!(e))?
            .segments;
        let has_camera = item.recording.as_ref().is_some_and(|r| r.has_camera);
        let fps = item.video.map_or(30.0, |v| v.fps).round().clamp(24.0, 60.0) as u32;
        let shared = SharedWgpuDevice {
            instance: gpu.instance.clone(),
            adapter: gpu.adapter.clone(),
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            is_software_adapter: false,
        };
        let constants =
            RenderVideoConstants::new_with_device(shared, &render_segments, recording_meta, studio)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let mut layers = RendererLayers::new_with_options(&gpu.device, &gpu.queue, false);
        let defaults = ProjectConfiguration::default();
        layers.preload_cursor_assets(&constants, defaults.cursor.use_svg, defaults.cursor.cursor_type());
        let session = CapRenderSession::new(&gpu.device, 16, 16);
        let segment_secs = item
            .recording
            .as_ref()
            .map(|r| r.segments.iter().map(|t| t.secs()).collect())
            .unwrap_or_default();
        Ok(Self {
            constants,
            layers,
            session,
            segments,
            segment_secs,
            has_camera,
            config: defaults,
            key: None,
            zoom: vec![],
            cursor: vec![],
            fps,
        })
    }

    fn total_secs(&self) -> f64 {
        self.segment_secs.iter().sum()
    }

    /// Cập nhật cấu hình khi hiệu ứng hoặc canvas đổi (dựng lại timeline zoom / con trỏ).
    fn ensure_config(&mut self, fx: &RecordingFx, canvas: (u32, u32)) {
        if self.key.as_ref().is_some_and(|(f, c)| f == fx && *c == canvas) {
            return;
        }
        self.config = fx_map::to_cap_config(fx, canvas, &self.segment_secs, self.has_camera);
        let smoothing = (!self.config.cursor.raw).then_some(SpringMassDamperSimulationConfig {
            tension: self.config.cursor.tension,
            mass: self.config.cursor.mass,
            friction: self.config.cursor.friction,
        });
        let click = self.config.cursor.click_spring_config();
        let total = self.total_secs();
        let screen = self.constants.options.screen_size;
        self.cursor = self
            .segments
            .iter()
            .map(|s| PrecomputedCursorTimeline::new(&s.cursor, smoothing, Some(click)))
            .collect();
        self.zoom = self
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| ZoomTransformTimeline::from_project_for_clip(&self.config, &s.cursor, total, screen, i as u32))
            .collect();
        self.key = Some((fx.clone(), canvas));
    }

    /// Frame tại thời điểm `t` TRONG MEDIA (các segment nối liền).
    pub async fn frame(
        &mut self,
        fx: &RecordingFx,
        canvas: (u32, u32),
        t: Ticks,
    ) -> anyhow::Result<GpuFrame> {
        self.ensure_config(fx, canvas);
        let secs = t.secs().max(0.0);
        // Segment chứa thời điểm này.
        let mut index = self.segments.len().saturating_sub(1);
        let mut acc = 0.0;
        for (i, d) in self.segment_secs.iter().enumerate() {
            if secs < acc + d {
                index = i;
                break;
            }
            acc += d;
        }
        let local = (secs - self.segment_secs[..index].iter().sum::<f64>()).max(0.0);
        let segment = &self.segments[index];
        let frames = segment
            .decoders
            .get_frames(local as f32, self.has_camera, true, Default::default())
            .await
            .ok_or_else(|| anyhow::anyhow!("không giải mã được frame tại {secs:.3}s"))?;

        let frame_number = (secs * self.fps as f64).round() as u32;
        self.zoom[index].ensure_precomputed_until((frame_number + 1) as f32 / self.fps as f32);
        let uniforms = ProjectUniforms::new_with_precomputed_cursor(
            &self.constants,
            &self.config,
            frame_number,
            self.fps,
            XY::new(canvas.0, canvas.1),
            &segment.cursor,
            &frames,
            self.total_secs(),
            &self.zoom[index],
            &self.cursor[index],
        );
        let tex = cap_rendering::render_frame_to_texture(
            &self.constants,
            &frames,
            &uniforms,
            &segment.cursor,
            &mut self.layers,
            &mut self.session,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(GpuFrame::whole(tex.create_view(&Default::default()), tex.width(), tex.height()))
    }
}
