//! Schema `project.json` (docs/plan-phase2.md mục 2.2). Đây là nguồn sự thật của timeline.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SCHEMA_VERSION: u32 = 1;

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
}

impl Clip {
    /// Độ dài trên timeline = (out - in) / speed.
    pub fn duration(&self) -> Ticks {
        Ticks(((self.source_out.0 - self.source_in.0) as f64 / self.speed).round() as i64)
    }
    pub fn end(&self) -> Ticks {
        self.start + self.duration()
    }
    /// Thời điểm trong media ứng với thời điểm `t` trên timeline.
    pub fn source_time_at(&self, t: Ticks) -> Ticks {
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
}

impl Default for Transform {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5, scale: 1.0, opacity: 1.0 }
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
}

impl ClipContent {
    pub fn is_overlay(&self) -> bool {
        matches!(self, Self::Text { .. } | Self::Blur { .. })
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
