//! `RecordingFx` (của Mochi) → `cap_project::ProjectConfiguration` (của Cap).
//!
//! Timeline đưa cho Cap là "đồng nhất": các segment bản quay nối tiếp nhau với tốc độ 1, nên thời
//! gian của Cap = thời gian TRONG MEDIA của bản quay. Cắt / tốc độ / vị trí trên timeline do
//! Mochi lo; Cap chỉ vẽ nền, khung, zoom, con trỏ, webcam tại một thời điểm trong media.

use cap_project::{
    AspectRatio, BackgroundBlurMode, BackgroundSource, BorderConfiguration, CameraShape as CapCameraShape,
    CameraXPosition, CameraYPosition, ChromaKeyConfig, CursorType, KeyboardData, KeyboardSettings,
    CornerStyle, GlideDirection, ProjectConfiguration, SceneMode, SceneSegment, ScreenMovementSpring,
    TimelineConfiguration, TimelineSegment, ZoomMode, ZoomSegment as CapZoom,
};
use mochi_project::{
    BackgroundKind, CameraBackground, CameraLayout, CameraShape, ClickEffect, Corner, CursorStyle, KeysPosition,
    RecordingFx, ZoomSpeed, ZoomTarget,
};

pub fn to_cap_config(
    fx: &RecordingFx,
    canvas: (u32, u32),
    segment_secs: &[f64],
    has_camera: bool,
) -> ProjectConfiguration {
    let mut c = ProjectConfiguration::default();
    c.aspect_ratio = aspect_for(canvas);

    // E07: nền + khung
    let bg = &fx.background;
    match bg.kind {
        BackgroundKind::None => {
            c.background.source = BackgroundSource::Color { value: [0, 0, 0], alpha: 255 };
            c.background.padding = 0.0;
            c.background.rounding = 0.0;
            c.background.shadow = 0.0;
        }
        BackgroundKind::Color => {
            c.background.source = BackgroundSource::Color { value: hex(&bg.color), alpha: 255 };
        }
        BackgroundKind::Gradient => {
            c.background.source = BackgroundSource::Gradient {
                from: hex(&bg.color),
                to: hex(&bg.color2),
                angle: bg.angle,
                noise_intensity: None,
                noise_scale: None,
                animated: None,
                animation_speed: None,
            };
        }
        BackgroundKind::Image => {
            c.background.source = BackgroundSource::Image {
                path: bg.image.as_ref().map(|p| p.display().to_string()),
            };
        }
    }
    if bg.kind != BackgroundKind::None {
        c.background.padding = bg.padding.clamp(0.0, 40.0);
        c.background.rounding = bg.rounding.clamp(0.0, 100.0);
        c.background.shadow = bg.shadow.clamp(0.0, 100.0) as f32;
    }
    c.screen_motion_blur = bg.motion_blur.clamp(0.0, 1.0) as f32;

    // E06: con trỏ
    c.cursor.hide = fx.cursor.hidden;
    c.cursor.hide_when_idle = fx.cursor.hide_when_idle;
    c.cursor.size = fx.cursor.size.clamp(50.0, 300.0).round() as u32;
    c.cursor.raw = !fx.cursor.smooth;
    c.cursor.ripple.enabled = fx.cursor.click_effect == ClickEffect::Ripple;
    // E06 (v1): nhòe khi di chuyển (tùy chọn, mặc định tắt) và bộ con trỏ thay thế (macOS / Windows).
    // Nghiêng con trỏ của Cap vẫn tắt (không có trong use case).
    c.cursor.motion_blur = fx.cursor.motion_blur.clamp(0.0, 1.0) as f32;
    c.cursor.rotation_amount = 0.0;
    c.cursor.base_rotation = 0.0;
    match fx.cursor.style {
        CursorStyle::Recorded => {
            c.cursor.use_svg = false;
            c.cursor.set_cursor_type(CursorType::Auto);
        }
        CursorStyle::MacOs => {
            c.cursor.use_svg = true;
            c.cursor.set_cursor_type(CursorType::MacOS);
        }
        CursorStyle::Windows => {
            c.cursor.use_svg = true;
            c.cursor.set_cursor_type(CursorType::Windows);
        }
    }
    // R13 / E06: highlight con trỏ (vá Cap `[MOCHI]`, vẽ dưới con trỏ, theo zoom, ẩn theo con trỏ).
    let h = fx.cursor.highlight.clone().clamped();
    let rgb = hex(&h.color);
    c.cursor.highlight = cap_project::CursorHighlightConfig {
        enabled: h.enabled,
        color: rgb,
        opacity: (h.opacity / 100.0) as f32,
        size: (h.size / 100.0) as f32,
    };

    // E08: webcam
    let cam = &fx.camera;
    c.camera.hide = !cam.visible || !has_camera;
    c.camera.mirror = cam.mirror;
    c.camera.size = cam.size.clamp(10.0, 50.0) as f32;
    // E08: vị trí tự do (renderer Cap kẹp để webcam luôn nằm trọn trong khung).
    c.camera.manual_position = cam.position.map(|p| cap_project::XY::new(p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0)));
    (c.camera.position.x, c.camera.position.y) = match cam.corner {
        Corner::TopLeft => (CameraXPosition::Left, CameraYPosition::Top),
        Corner::TopRight => (CameraXPosition::Right, CameraYPosition::Top),
        Corner::BottomLeft => (CameraXPosition::Left, CameraYPosition::Bottom),
        Corner::BottomRight => (CameraXPosition::Right, CameraYPosition::Bottom),
    };
    match cam.shape {
        CameraShape::Circle => {
            c.camera.shape = CapCameraShape::Square;
            c.camera.rounding = 100.0;
            c.camera.rounding_type = CornerStyle::Rounded;
        }
        CameraShape::Rounded => {
            c.camera.shape = CapCameraShape::Square;
            c.camera.rounding = 30.0;
            c.camera.rounding_type = CornerStyle::Squircle;
        }
        CameraShape::Source => {
            c.camera.shape = CapCameraShape::Source;
            c.camera.rounding = 20.0;
        }
    }

    // E08 (v1): nền webcam (tách người bằng model chạy trên máy) + chroma key.
    c.camera.background_blur.mode = match cam.background {
        CameraBackground::Keep => BackgroundBlurMode::Off,
        CameraBackground::Blur => BackgroundBlurMode::Light,
        CameraBackground::StrongBlur => BackgroundBlurMode::Heavy,
        CameraBackground::Remove => BackgroundBlurMode::Remove,
    };
    c.camera.chroma_key = cam.chroma.enabled.then(|| ChromaKeyConfig {
        color: hex(&cam.chroma.color),
        tolerance: (cam.chroma.tolerance / 100.0).clamp(0.01, 1.0) as f32,
    });

    // R09 (v1): overlay phím bấm từ dữ liệu đã ghi.
    if fx.keys.enabled {
        let mut settings = KeyboardSettings::default();
        settings.enabled = true;
        settings.size = fx.keys.size.clamp(20.0, 120.0) as u32;
        settings.position = match fx.keys.position {
            KeysPosition::BottomCenter => "bottom-center",
            KeysPosition::BottomLeft => "bottom-left",
            KeysPosition::BottomRight => "bottom-right",
            KeysPosition::TopCenter => "top-center",
        }
        .into();
        c.keyboard = Some(KeyboardData { settings });
    }

    c.camera.border = Some(BorderConfiguration {
        enabled: cam.border,
        width: cam.border_width.clamp(0.5, 10.0) as f32,
        color: hex(&cam.border_color),
        opacity: 100.0,
    });

    // E05: độ mượt chuyển động zoom
    c.screen_movement_spring = match fx.zoom.speed {
        ZoomSpeed::Slow => ScreenMovementSpring { stiffness: 110.0, damping: 38.0, mass: 3.0 },
        ZoomSpeed::Mellow => ScreenMovementSpring::default(),
        ZoomSpeed::Quick => ScreenMovementSpring { stiffness: 340.0, damping: 42.0, mass: 1.6 },
    };

    // Timeline đồng nhất + zoom + đoạn ẩn webcam (đều theo thời gian trong media).
    let segments = segment_secs
        .iter()
        .enumerate()
        .scan(0.0f64, |acc, (i, d)| {
            let seg = TimelineSegment {
                recording_clip: i as u32,
                timescale: 1.0,
                start: 0.0,
                end: *d,
                name: None,
                speed_audio_mode: None,
                volume: None,
                hide_cursor: None,
            };
            *acc += d;
            Some(seg)
        })
        .collect();
    let zoom_segments = fx
        .zoom
        .segments
        .iter()
        .filter(|z| z.end > z.start)
        .map(|z| CapZoom {
            start: z.start.secs(),
            end: z.end.secs(),
            amount: z.amount.clamp(1.0, 5.0),
            mode: match z.target {
                ZoomTarget::Follow => ZoomMode::Auto,
                ZoomTarget::Fixed { x, y } => ZoomMode::Manual {
                    x: x.clamp(0.0, 1.0) as f32,
                    y: y.clamp(0.0, 1.0) as f32,
                },
            },
            glide_direction: GlideDirection::None,
            glide_speed: 0.5,
            instant_animation: false,
            edge_snap_ratio: 0.25,
        })
        .collect();
    // E08: bố cục cả clip (v1) + các đoạn ẩn webcam (ẩn thắng bố cục), không chồng nhau.
    let total: f64 = segment_secs.iter().sum();
    let mut hidden: Vec<(f64, f64)> = cam
        .hidden_ranges
        .iter()
        .filter(|r| r.end > r.start)
        .map(|r| (r.start.secs().max(0.0), r.end.secs().min(total.max(r.end.secs()))))
        .collect();
    hidden.sort_by(|a, b| a.0.total_cmp(&b.0));
    let scene = |start: f64, end: f64, mode: SceneMode| SceneSegment {
        start,
        end,
        mode,
        split_layout: None,
        transition_in: 0.25,
        transition_out: 0.25,
    };
    let base = match cam.layout {
        CameraLayout::Pip => None,
        CameraLayout::CameraOnly => Some(SceneMode::CameraOnly),
        CameraLayout::Split => Some(SceneMode::SplitScreen),
        CameraLayout::Floating => Some(SceneMode::Floating),
    };
    let mut scene_segments = Vec::new();
    let mut at = 0.0;
    for (s, e) in &hidden {
        if let Some(mode) = base
            && *s > at
        {
            scene_segments.push(scene(at, *s, mode));
        }
        scene_segments.push(scene(*s, *e, SceneMode::HideCamera));
        at = at.max(*e);
    }
    if let Some(mode) = base
        && total > at
    {
        scene_segments.push(scene(at, total, mode));
    }
    let mut timeline = c.timeline.take().unwrap_or_else(empty_timeline);
    timeline.segments = segments;
    timeline.zoom_segments = zoom_segments;
    timeline.scene_segments = scene_segments;
    c.timeline = Some(timeline);
    c
}

fn empty_timeline() -> TimelineConfiguration {
    serde_json::from_value(serde_json::json!({ "segments": [], "zoomSegments": [] }))
        .expect("timeline rỗng hợp lệ")
}

/// Tỉ lệ khung Cap gần nhất với canvas (E09). Không khớp tỉ lệ nào → để Cap tự theo bản quay.
fn aspect_for((w, h): (u32, u32)) -> Option<AspectRatio> {
    let r = w as f64 / h.max(1) as f64;
    [
        (16.0 / 9.0, AspectRatio::Wide),
        (9.0 / 16.0, AspectRatio::Vertical),
        (1.0, AspectRatio::Square),
        (4.0 / 3.0, AspectRatio::Classic),
        (3.0 / 4.0, AspectRatio::Tall),
    ]
    .into_iter()
    .find(|(a, _)| (a - r).abs() < 0.02)
    .map(|(_, ar)| ar)
}

fn hex(s: &str) -> [u16; 3] {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).unwrap_or(0);
    [((v >> 16) & 0xff) as u16, ((v >> 8) & 0xff) as u16, (v & 0xff) as u16]
}
