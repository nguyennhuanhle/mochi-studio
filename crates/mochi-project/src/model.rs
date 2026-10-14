//! Schema `project.json` (docs/plan-phase2.md mục 2.2). Đây là nguồn sự thật của timeline.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// v2 (Phase 6, v1): keyframe, transition, màu / viền / khung thiết bị, freeze frame, hình khối,
/// callout, spotlight, marker, phụ đề, hiệu ứng âm thanh. Mọi trường mới có mặc định → v1 đọc được.
pub const SCHEMA_VERSION: u32 = 2;

/// 705.600.000 tick/giây (như Camtasia): chia hết cho mọi fps và sample rate thông dụng.
pub const TICKS_PER_SECOND: i64 = 705_600_000;

/// Thời gian theo tick. Sang TS là `number`: 2^53 tick ≈ 147 ngày, đủ cho mọi project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct Ticks(#[specta(type = f64)] pub i64);

impl Ticks {
    pub const ZERO: Ticks = Ticks(0);

    pub fn from_secs(s: f64) -> Self {
        Ticks((s * TICKS_PER_SECOND as f64).round() as i64)
    }
    pub fn secs(self) -> f64 {
        self.0 as f64 / TICKS_PER_SECOND as f64
    }
}

impl std::ops::Add for Ticks {
    type Output = Ticks;
    fn add(self, o: Ticks) -> Ticks {
        Ticks(self.0 + o.0)
    }
}
impl std::ops::Sub for Ticks {
    type Output = Ticks;
    fn sub(self, o: Ticks) -> Ticks {
        Ticks(self.0 - o.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub app_version: String,
    pub canvas: Canvas,
    pub media: Vec<MediaItem>,
    /// Thứ tự = z-order: phần tử đầu ở dưới cùng.
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    /// E14 (v1): marker trên timeline.
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// E13 (v1): phụ đề (thời gian trên timeline).
    #[serde(default)]
    pub captions: Vec<Caption>,
    #[serde(default)]
    pub caption_style: CaptionStyle,
}

/// E14: marker — một mốc có tên trên timeline (nhảy tới, xuất mỗi marker một file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub id: String,
    pub at: Ticks,
    pub label: String,
}

/// E13: một dòng phụ đề. `words` (nếu có) cho phụ đề động hiện từng chữ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Caption {
    pub id: String,
    pub start: Ticks,
    pub end: Ticks,
    pub text: String,
    #[serde(default)]
    pub words: Vec<CaptionWord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CaptionWord {
    pub start: Ticks,
    pub end: Ticks,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CaptionStyle {
    /// Hiện phụ đề trong preview / video xuất (burn-in, X05).
    pub visible: bool,
    pub font: String,
    /// Cỡ chữ theo % chiều cao canvas (2–10).
    pub size: f64,
    pub color: String,
    /// Nền sau chữ; None = không nền.
    pub background: Option<String>,
    /// Vị trí theo chiều dọc (0 = trên, 1 = dưới).
    pub y: f64,
    /// Phụ đề động: tô màu từng chữ đang nói.
    pub animated: bool,
    pub highlight: String,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            visible: true,
            font: String::new(),
            size: 4.5,
            color: "#FFFFFF".into(),
            background: Some("#000000B0".into()),
            y: 0.88,
            animated: false,
            highlight: "#FFD54A".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for Canvas {
    fn default() -> Self {
        Self { width: 1920, height: 1080, fps: 30 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Recording,
    Video,
    Audio,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub kind: MediaKind,
    pub name: String,
    /// Đường dẫn tuyệt đối lúc import.
    pub abs_path: PathBuf,
    /// Đường dẫn tương đối so với gói `.mochi` (nếu tính được) — để relink khi chuyển máy.
    pub rel_path: Option<String>,
    /// xxh3(kích thước + 1 MB đầu + 1 MB cuối), hex.
    pub hash: String,
    pub size_bytes: f64,
    pub duration: Ticks,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
    pub recording: Option<RecordingInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub channels: u32,
    pub sample_rate: u32,
}

/// Bản quay của Mochi (layout Cap) trong `recordings/<id>/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordingInfo {
    /// Thời lượng từng segment (nối liền thành một media).
    pub segments: Vec<Ticks>,
    pub has_cursor: bool,
    pub has_camera: bool,
    pub has_mic: bool,
    pub has_system_audio: bool,
    /// Khoảng mất webcam / mic / âm thanh hệ thống khi đang quay (R07), giây trên bản quay.
    #[serde(default)]
    pub lost: Vec<crate::sidecar::LostRange>,
    /// Cửa sổ đang active theo thời gian (R03).
    #[serde(default)]
    pub active_windows: Vec<crate::sidecar::ActiveWindow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Video,
    Audio,
    Overlay,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub kind: TrackKind,
    pub name: String,
    pub locked: bool,
    pub muted: bool,
    pub hidden: bool,
    /// E11: âm lượng của cả track (dB, cộng với âm lượng từng clip). -60…+12.
    #[serde(default)]
    pub volume_db: f64,
    /// E14 (v1): track nam châm — xóa / dời clip thì các clip sau tự dồn lại, không để khoảng trống.
    #[serde(default)]
    pub magnetic: bool,
    /// E11 (v1): ducking — track này (nhạc nền) tự nhỏ đi khi các track khác có tiếng nói.
    #[serde(default)]
    pub duck: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    pub track_id: String,
    pub media_id: Option<String>,
    /// Vị trí trên timeline.
    pub start: Ticks,
    /// Đoạn lấy từ media.
    pub source_in: Ticks,
    pub source_out: Ticks,
    /// 0.25–4 (E04).
    pub speed: f64,
    pub volume_db: f64,
    pub fade_in: Ticks,
    pub fade_out: Ticks,
    pub muted: bool,
    pub transform: Option<Transform>,
    pub content: ClipContent,
    /// E04 (v1): freeze frame — clip đứng yên ở thời điểm này trong media (không tiếng).
    #[serde(default)]
    pub hold: Option<Ticks>,
    /// E12 (v1): keyframe vị trí / scale / xoay / độ mờ (thời gian tính từ đầu clip). Rỗng = `transform`.
    #[serde(default)]
    pub keyframes: Vec<Keyframe>,
    /// E12 (v1): chuyển cảnh khi clip vào / ra.
    #[serde(default)]
    pub transition_in: Option<Transition>,
    #[serde(default)]
    pub transition_out: Option<Transition>,
    /// E15 (v1): màu cơ bản, viền, bo góc, khung thiết bị.
    #[serde(default)]
    pub look: Look,
    /// E11 (v1): khử ồn, chuẩn hóa độ to, nén.
    #[serde(default)]
    pub audio_fx: AudioFx,
    /// E14 (v1): nhóm — các clip cùng nhóm di chuyển / xóa cùng nhau.
    #[serde(default)]
    pub group: Option<String>,
}

/// E12: một keyframe. `at` tính từ đầu clip; giữa hai keyframe nội suy theo `easing` của keyframe sau.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Keyframe {
    pub at: Ticks,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    /// Độ (theo chiều kim đồng hồ).
    pub rotation: f64,
    pub opacity: f64,
    pub easing: Easing,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
}

impl Easing {
    pub fn apply(self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Easing::Linear => u,
            Easing::EaseIn => u * u * u,
            Easing::EaseOut => 1.0 - (1.0 - u).powi(3),
            Easing::EaseInOut => {
                if u < 0.5 {
                    4.0 * u * u * u
                } else {
                    1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

/// E12: chuyển cảnh. Ở đầu clip mà ngay trước có clip khác trên cùng track → chuyển giữa hai clip;
/// không có → hiện dần từ lớp bên dưới. Ở cuối clip → ẩn dần.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration: Ticks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TransitionKind {
    Fade,
    FadeBlack,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    WipeLeft,
    WipeRight,
    Zoom,
    /// Chỉ cho chữ / callout: hiện từng ký tự.
    Typewriter,
}

/// E15: chỉnh màu cơ bản + viền + bo góc + khung thiết bị, áp cho mọi clip có hình.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Look {
    /// −100…100 (0 = giữ nguyên).
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    /// Độ dày viền, pixel canvas 0–40.
    pub border_width: f64,
    pub border_color: String,
    /// Bo góc, % cạnh ngắn của clip 0–50.
    pub corner_radius: f64,
    /// Đổ bóng 0–100.
    pub shadow: f64,
    pub device: DeviceFrame,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            border_width: 0.0,
            border_color: "#FFFFFF".into(),
            corner_radius: 0.0,
            shadow: 0.0,
            device: DeviceFrame::None,
        }
    }
}

impl Look {
    pub fn is_identity(&self) -> bool {
        *self == Look::default()
    }

    pub fn clamped(mut self) -> Self {
        let f = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.brightness = f(self.brightness, -100.0, 100.0, 0.0);
        self.contrast = f(self.contrast, -100.0, 100.0, 0.0);
        self.saturation = f(self.saturation, -100.0, 100.0, 0.0);
        self.border_width = f(self.border_width, 0.0, 40.0, 0.0);
        self.corner_radius = f(self.corner_radius, 0.0, 50.0, 0.0);
        self.shadow = f(self.shadow, 0.0, 100.0, 0.0);
        if !is_hex_color(&self.border_color) {
            self.border_color = "#FFFFFF".into();
        }
        self
    }
}

/// "#RRGGBB" hoặc "#RRGGBBAA".
pub fn is_hex_color(c: &str) -> bool {
    (c.len() == 7 || c.len() == 9) && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceFrame {
    #[default]
    None,
    Laptop,
    Phone,
    Browser,
}

/// E11 (v1): xử lý tiếng của clip (chạy trên máy, kết quả cache trong `cache/`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioFx {
    /// Khử ồn (RNNoise).
    pub denoise: bool,
    /// Chuẩn hóa độ to về −16 LUFS.
    pub normalize: bool,
    /// Nén (compressor) cho giọng nói đều hơn.
    pub compress: bool,
}

impl Clip {
    /// Biến đổi tại `t` (timeline): nội suy keyframe nếu có, không thì `transform` tĩnh.
    pub fn transform_at(&self, t: Ticks) -> Transform {
        let base = self.transform.unwrap_or_default();
        if self.keyframes.is_empty() {
            return base;
        }
        let local = t - self.start;
        let mut ks = self.keyframes.clone();
        ks.sort_by_key(|k| k.at);
        let first = ks[0];
        let last = *ks.last().unwrap();
        let tf = |k: &Keyframe| Transform { x: k.x, y: k.y, scale: k.scale, opacity: k.opacity, rotation: k.rotation };
        if local <= first.at {
            return tf(&first);
        }
        if local >= last.at {
            return tf(&last);
        }
        let i = ks.iter().position(|k| k.at > local).unwrap_or(ks.len() - 1);
        let (a, b) = (ks[i - 1], ks[i]);
        let span = (b.at.0 - a.at.0).max(1) as f64;
        let u = b.easing.apply((local.0 - a.at.0) as f64 / span);
        let lerp = |p: f64, q: f64| p + (q - p) * u;
        Transform {
            x: lerp(a.x, b.x),
            y: lerp(a.y, b.y),
            scale: lerp(a.scale, b.scale),
            rotation: lerp(a.rotation, b.rotation),
            opacity: lerp(a.opacity, b.opacity),
        }
    }

    /// Độ dài trên timeline = (out - in) / speed.
    pub fn duration(&self) -> Ticks {
        Ticks(((self.source_out.0 - self.source_in.0) as f64 / self.speed).round() as i64)
    }
    pub fn end(&self) -> Ticks {
        self.start + self.duration()
    }
    /// Thời điểm trong media ứng với thời điểm `t` trên timeline (freeze frame → luôn `hold`).
    pub fn source_time_at(&self, t: Ticks) -> Ticks {
        if let Some(h) = self.hold {
            return h;
        }
        self.source_in + Ticks(((t.0 - self.start.0) as f64 * self.speed).round() as i64)
    }
}

/// Biến đổi tĩnh ở MVP (keyframe ở v1). x, y: tâm theo tỉ lệ canvas (0.5 = giữa).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub opacity: f64,
    /// E12 (v1): xoay, độ (theo chiều kim đồng hồ).
    #[serde(default)]
    pub rotation: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5, scale: 1.0, opacity: 1.0, rotation: 0.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClipContent {
    Recording {
        #[serde(default)]
        fx: RecordingFx,
    },
    Video,
    Audio,
    Image,
    #[serde(rename_all = "camelCase")]
    Text {
        text: String,
        font: String,
        size_px: f64,
        color: String,
        background: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Blur {
        /// Hình chữ nhật chuẩn hóa 0–1 trên canvas.
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        pixelate: bool,
        strength: f64,
    },
    /// E10 (v1): hình khối / mũi tên / đường kẻ. (x0, y0) → (x1, y1) chuẩn hóa 0–1 trên canvas.
    #[serde(rename_all = "camelCase")]
    Shape {
        shape: ShapeKind,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        stroke: String,
        /// Độ dày nét, pixel canvas.
        stroke_width: f64,
        fill: Option<String>,
    },
    /// E10 (v1): callout — hộp chữ có đuôi chỉ vào (tail_x, tail_y).
    #[serde(rename_all = "camelCase")]
    Callout {
        text: String,
        font: String,
        size_px: f64,
        color: String,
        background: String,
        tail_x: f64,
        tail_y: f64,
    },
    /// E10 (v1): spotlight vùng — làm tối mọi thứ ngoài vùng sáng.
    #[serde(rename_all = "camelCase")]
    Spotlight {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ellipse: bool,
        /// 0–1: độ tối phần ngoài.
        dim: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Arrow,
    Line,
}

impl ClipContent {
    pub fn is_overlay(&self) -> bool {
        matches!(
            self,
            Self::Text { .. } | Self::Blur { .. } | Self::Shape { .. } | Self::Callout { .. } | Self::Spotlight { .. }
        )
    }
}

/// Hiệu ứng gắn với clip quay (E05–E09). `mochi-render` dịch sang `cap_project::ProjectConfiguration`
/// để renderer của Cap vẽ. Giá trị mặc định = bộ "auto-polish" (R08).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordingFx {
    pub background: BackgroundFx,
    pub cursor: CursorFx,
    pub camera: CameraFx,
    pub zoom: ZoomFx,
    /// R09 / E10 (v1): hiện phím tắt đã ghi lên video.
    pub keys: KeysFx,
}

/// R09 / E10 (v1): overlay phím bấm (từ dữ liệu ghi phím của bản quay).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct KeysFx {
    pub enabled: bool,
    pub position: KeysPosition,
    /// Cỡ chữ (pixel ở 1080p): 20–120.
    pub size: f64,
}

impl Default for KeysFx {
    fn default() -> Self {
        Self { enabled: false, position: KeysPosition::BottomCenter, size: 50.0 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KeysPosition {
    #[default]
    BottomCenter,
    BottomLeft,
    BottomRight,
    TopCenter,
}

impl RecordingFx {
    /// Kẹp mọi giá trị về giới hạn (ghi vào project, không chỉ lúc render). Giới hạn trùng với UI.
    pub fn clamped(mut self) -> Self {
        fn hex(c: &mut String, fallback: &str) {
            let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
            if !ok {
                *c = fallback.into();
            }
        }
        let d = Self::default();
        let b = &mut self.background;
        b.angle %= 360;
        b.padding = clampf(b.padding, 0.0, 40.0);
        b.rounding = clampf(b.rounding, 0.0, 100.0);
        b.shadow = clampf(b.shadow, 0.0, 100.0);
        b.motion_blur = clampf(b.motion_blur, 0.0, 1.0);
        hex(&mut b.color, &d.background.color);
        hex(&mut b.color2, &d.background.color2);
        self.cursor.size = clampf(self.cursor.size, 50.0, 300.0);
        self.cursor.highlight = std::mem::take(&mut self.cursor.highlight).clamped();
        self.cursor.motion_blur = clampf(self.cursor.motion_blur, 0.0, 1.0);
        let s = &mut self.cursor.spotlight;
        s.size = clampf(s.size, 5.0, 60.0);
        s.dim = clampf(s.dim, 0.0, 1.0);
        let m = &mut self.cursor.magnifier;
        m.zoom = clampf(m.zoom, 1.5, 4.0);
        m.size = clampf(m.size, 8.0, 40.0);
        self.cursor.click_sound.volume = clampf(self.cursor.click_sound.volume, 0.0, 100.0);
        self.keys.size = clampf(self.keys.size, 20.0, 120.0);
        self.camera.chroma.tolerance = clampf(self.camera.chroma.tolerance, 1.0, 100.0);
        hex(&mut self.camera.chroma.color, "#00B140");
        let c = &mut self.camera;
        c.size = clampf(c.size, 10.0, 50.0);
        c.border_width = clampf(c.border_width, 0.5, 10.0);
        if let Some(p) = &mut c.position {
            p.x = clampf(p.x, 0.0, 1.0);
            p.y = clampf(p.y, 0.0, 1.0);
        }
        c.hidden_ranges.retain(|r| r.end > r.start);
        hex(&mut c.border_color, &d.camera.border_color);
        for z in &mut self.zoom.segments {
            z.amount = clampf(z.amount, 1.0, 5.0);
            if let ZoomTarget::Fixed { x, y } = &mut z.target {
                *x = clampf(*x, 0.0, 1.0);
                *y = clampf(*y, 0.0, 1.0);
            }
        }
        self
    }
}

/// Kẹp; NaN (JSON sửa tay) → giới hạn dưới.
pub fn clampf(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { lo } else { v.clamp(lo, hi) }
}

impl Default for RecordingFx {
    fn default() -> Self {
        Self {
            background: BackgroundFx::default(),
            cursor: CursorFx::default(),
            camera: CameraFx::default(),
            zoom: ZoomFx::default(),
            keys: KeysFx::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundKind {
    /// Không có khung: bản quay phủ kín.
    None,
    Color,
    Gradient,
    Image,
}

/// E07: nền + khung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct BackgroundFx {
    pub kind: BackgroundKind,
    /// "#RRGGBB"
    pub color: String,
    pub color2: String,
    /// Góc gradient (độ).
    pub angle: u16,
    pub image: Option<PathBuf>,
    /// 0–40 (% cạnh ngắn).
    pub padding: f64,
    /// 0–100.
    pub rounding: f64,
    /// 0–100.
    pub shadow: f64,
    /// 0–1, nhòe chuyển động khi zoom/pan.
    pub motion_blur: f64,
}

impl Default for BackgroundFx {
    fn default() -> Self {
        Self {
            kind: BackgroundKind::Gradient,
            color: "#1D1B26".into(),
            color2: "#E2557B".into(),
            angle: 135,
            image: None,
            padding: 8.0,
            rounding: 35.0,
            shadow: 70.0,
            motion_blur: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ClickEffect {
    None,
    Ripple,
}

/// E06: con trỏ (vẽ lại từ dữ liệu con trỏ, không dính trong hình).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CursorFx {
    pub hidden: bool,
    pub hide_when_idle: bool,
    /// Phần trăm kích thước: 50–300.
    pub size: f64,
    /// Làm mượt đường đi.
    pub smooth: bool,
    pub click_effect: ClickEffect,
    /// R13 / E06 (v1.3): vòng sáng quanh con trỏ.
    pub highlight: CursorHighlight,
    /// E06 (v1): kiểu con trỏ — đúng như đã ghi, hoặc thay bằng bộ con trỏ macOS / Windows.
    pub style: CursorStyle,
    /// E06 (v1): nhòe con trỏ khi di chuyển nhanh, 0–1 (0 = tắt).
    pub motion_blur: f64,
    /// E06 (v1): làm tối mọi thứ ngoài vùng quanh con trỏ.
    pub spotlight: CursorSpotlight,
    /// E06 (v1): kính lúp quanh con trỏ.
    pub magnifier: CursorMagnifier,
    /// E06 (v1): tiếng "tách" mỗi lần click.
    pub click_sound: ClickSound,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Recorded,
    MacOs,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CursorSpotlight {
    pub enabled: bool,
    /// Bán kính vùng sáng, % cạnh ngắn của khung: 5–60.
    pub size: f64,
    /// Độ tối phần ngoài 0–1.
    pub dim: f64,
}

impl Default for CursorSpotlight {
    fn default() -> Self {
        Self { enabled: false, size: 16.0, dim: 0.55 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CursorMagnifier {
    pub enabled: bool,
    /// Độ phóng 1,5–4×.
    pub zoom: f64,
    /// Bán kính kính lúp, % cạnh ngắn của khung: 8–40.
    pub size: f64,
}

impl Default for CursorMagnifier {
    fn default() -> Self {
        Self { enabled: false, zoom: 2.0, size: 16.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct ClickSound {
    pub enabled: bool,
    /// 0–100 %.
    pub volume: f64,
}

impl Default for ClickSound {
    fn default() -> Self {
        Self { enabled: false, volume: 60.0 }
    }
}

/// R13 / E06: highlight con trỏ kiểu Camtasia — chỉ là hiệu ứng khi render (không dính vào bản quay).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CursorHighlight {
    pub enabled: bool,
    /// "#RRGGBB"
    pub color: String,
    /// Bán kính, % chiều cao con trỏ: 50–500.
    pub size: f64,
    /// Độ đậm ở tâm, %: 10–100.
    pub opacity: f64,
}

impl Default for CursorHighlight {
    fn default() -> Self {
        Self { enabled: false, color: "#FFD54A".into(), size: 160.0, opacity: 45.0 }
    }
}

impl CursorHighlight {
    pub fn clamped(mut self) -> Self {
        self.size = clampf(self.size, 50.0, 500.0);
        self.opacity = clampf(self.opacity, 10.0, 100.0);
        let ok = self.color.len() == 7 && self.color.starts_with('#') && self.color[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !ok {
            self.color = Self::default().color;
        }
        self
    }
}

impl Default for CursorFx {
    fn default() -> Self {
        Self {
            hidden: false,
            hide_when_idle: true,
            size: 150.0,
            smooth: true,
            click_effect: ClickEffect::Ripple,
            highlight: CursorHighlight::default(),
            style: CursorStyle::Recorded,
            motion_blur: 0.0,
            spotlight: CursorSpotlight::default(),
            magnifier: CursorMagnifier::default(),
            click_sound: ClickSound::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraShape {
    Rounded,
    Circle,
    /// Giữ tỉ lệ gốc của webcam.
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Điểm chuẩn hóa 0–1 theo khung.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NormPoint {
    pub x: f64,
    pub y: f64,
}

/// Một khoảng thời gian trong media (source).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TimeRange {
    pub start: Ticks,
    pub end: Ticks,
}

/// E08: webcam dạng picture-in-picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CameraFx {
    pub visible: bool,
    pub shape: CameraShape,
    pub corner: Corner,
    /// Vị trí tự do (tâm webcam, 0–1 theo khung). Có giá trị thì bỏ qua `corner`.
    pub position: Option<NormPoint>,
    /// Phần trăm cạnh ngắn của khung: 10–50.
    pub size: f64,
    pub mirror: bool,
    /// Những đoạn (thời gian trong media) ẩn webcam.
    pub hidden_ranges: Vec<TimeRange>,
    /// Viền quanh webcam.
    pub border: bool,
    /// Màu viền dạng `#RRGGBB`.
    pub border_color: String,
    /// Độ dày viền, phần trăm cạnh ngắn của webcam: 0,5–10.
    pub border_width: f64,
    /// E08 (v1): bố cục — webcam góc (PiP), chỉ webcam, chia đôi, hai thẻ nổi.
    pub layout: CameraLayout,
    /// E08 (v1): nền phía sau người trong webcam (AI chạy trên máy).
    pub background: CameraBackground,
    /// E08 (v1): chroma key (phông xanh).
    pub chroma: ChromaKey,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraLayout {
    #[default]
    Pip,
    CameraOnly,
    Split,
    Floating,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraBackground {
    #[default]
    Keep,
    Blur,
    StrongBlur,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct ChromaKey {
    pub enabled: bool,
    pub color: String,
    /// Ngưỡng 1–100.
    pub tolerance: f64,
}

impl Default for ChromaKey {
    fn default() -> Self {
        Self { enabled: false, color: "#00B140".into(), tolerance: 30.0 }
    }
}

impl Default for CameraFx {
    fn default() -> Self {
        Self {
            visible: true,
            shape: CameraShape::Circle,
            corner: Corner::BottomRight,
            position: None,
            size: 26.0,
            mirror: true,
            hidden_ranges: vec![],
            border: false,
            border_color: "#FFFFFF".into(),
            border_width: 3.0,
            layout: CameraLayout::Pip,
            background: CameraBackground::Keep,
            chroma: ChromaKey::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ZoomSpeed {
    Slow,
    Mellow,
    Quick,
}

/// E05: zoom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct ZoomFx {
    pub segments: Vec<ZoomSegment>,
    pub speed: ZoomSpeed,
    /// R08: lý do không tạo được zoom tự động lúc dừng quay (UI báo + cho thử lại).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_error: Option<String>,
}

impl Default for ZoomFx {
    fn default() -> Self {
        Self { segments: vec![], speed: ZoomSpeed::Mellow, auto_error: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ZoomTarget {
    /// Bám theo con trỏ.
    Follow,
    /// Zoom vào một điểm cố định (0–1 theo khung bản quay).
    Fixed { x: f64, y: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ZoomSegment {
    pub id: String,
    /// Thời gian TRONG MEDIA (source): trim/cắt/di chuyển clip thì zoom vẫn bám đúng hình.
    pub start: Ticks,
    pub end: Ticks,
    /// 1–5.
    pub amount: f64,
    pub target: ZoomTarget,
    /// Sinh tự động (S04). Người dùng sửa thì thành false; "sinh lại" chỉ thay vùng auto.
    pub auto: bool,
}

#[cfg(test)]
mod clamp_tests {
    use super::*;

    #[test]
    fn recording_fx_is_clamped_in_project() {
        let mut fx = RecordingFx::default();
        fx.background.padding = 999.0;
        fx.background.shadow = f64::NAN;
        fx.background.color = "red".into();
        fx.cursor.size = 5.0;
        fx.camera.size = 80.0;
        fx.camera.border_width = -1.0;
        let c = fx.clamped();
        assert_eq!(c.background.padding, 40.0);
        assert_eq!(c.background.shadow, 0.0);
        assert_eq!(c.background.color, RecordingFx::default().background.color);
        assert_eq!(c.cursor.size, 50.0);
        assert_eq!(c.camera.size, 50.0);
        assert_eq!(c.camera.border_width, 0.5);
    }
}
