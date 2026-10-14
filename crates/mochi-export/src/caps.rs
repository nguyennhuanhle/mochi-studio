//! K-X3: kiểm encoder máy hỗ trợ. Mỗi codec thử lần lượt encoder phần cứng rồi phần mềm, mở thật
//! và mã hóa thử một frame (driver có thể khai báo encoder nhưng không chạy được).

use crate::encode::VideoEncoder;
use crate::settings::VideoCodec;
use serde::Serialize;
use std::sync::OnceLock;

/// Thứ tự ưu tiên: NVENC → QSV → AMF → Media Foundation → phần mềm.
/// HEVC / AV1 chỉ dùng phần cứng (X01: "nếu GPU hỗ trợ").
pub fn candidates(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["h264_nvenc", "h264_qsv", "h264_amf", "h264_mf", "libx264"],
        VideoCodec::Hevc => &["hevc_nvenc", "hevc_qsv", "hevc_amf", "hevc_mf"],
        VideoCodec::Av1 => &["av1_nvenc", "av1_qsv", "av1_amf"],
    }
}

pub fn is_hardware(encoder: &str) -> bool {
    !encoder.starts_with("lib")
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodecSupport {
    pub codec: VideoCodec,
    /// Encoder sẽ dùng (tốt nhất trong số chạy được). `None` = máy không hỗ trợ.
    pub encoder: Option<String>,
    pub hardware: bool,
    /// Lý do không hỗ trợ, để UI hiện cạnh lựa chọn bị mờ.
    pub reason: Option<String>,
    /// Mọi encoder chạy được của codec này, theo thứ tự ưu tiên.
    pub working: Vec<String>,
}

/// Biến môi trường kiểm thử: `MOCHI_TEST_DISABLE_ENCODERS=hevc_nvenc,av1_nvenc` coi như máy không có.
fn disabled_by_test(name: &str) -> bool {
    mochi_project::test_env("MOCHI_TEST_DISABLE_ENCODERS")
        .is_some_and(|v| v.split(',').any(|d| d.trim() == name))
}

fn probe(name: &str) -> Result<(), String> {
    if disabled_by_test(name) {
        return Err("tắt bởi MOCHI_TEST_DISABLE_ENCODERS".into());
    }
    if ffmpeg::encoder::find_by_name(name).is_none() {
        return Err("FFmpeg không có encoder này".into());
    }
    VideoEncoder::self_test(name)
}

fn detect() -> Vec<CodecSupport> {
    let _ = ffmpeg::init();
    // Thử encoder thất bại là chuyện bình thường; đừng để FFmpeg in đầy stderr.
    let level = ffmpeg::util::log::get_level().ok();
    ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Fatal);
    let result = VideoCodec::ALL
        .iter()
        .map(|&codec| {
            let mut working = Vec::new();
            let mut reasons = Vec::new();
            for name in candidates(codec) {
                match probe(name) {
                    Ok(()) => working.push(name.to_string()),
                    Err(e) => reasons.push(format!("{name}: {e}")),
                }
            }
            let encoder = working.first().cloned();
            CodecSupport {
                codec,
                hardware: encoder.as_deref().is_some_and(is_hardware),
                reason: encoder.is_none().then(|| match codec {
                    VideoCodec::H264 => "không có encoder H.264 nào chạy được".to_string(),
                    _ => "GPU của máy không có encoder phần cứng cho codec này".to_string(),
                }),
                encoder,
                working,
            }
            .log(&reasons)
        })
        .collect();
    if let Some(l) = level {
        ffmpeg::util::log::set_level(l);
    }
    result
}

impl CodecSupport {
    fn log(self, reasons: &[String]) -> Self {
        tracing::info!(codec = ?self.codec, encoder = ?self.encoder, "kiểm encoder; bị loại: {}", reasons.join("; "));
        self
    }
}

static CAPS: OnceLock<Vec<CodecSupport>> = OnceLock::new();

/// Kết quả kiểm encoder (mất ~1 s lần đầu, sau đó dùng lại).
pub fn encoder_capabilities() -> &'static [CodecSupport] {
    CAPS.get_or_init(detect)
}

pub fn support_for(codec: VideoCodec) -> Option<&'static CodecSupport> {
    encoder_capabilities().iter().find(|c| c.codec == codec)
}
