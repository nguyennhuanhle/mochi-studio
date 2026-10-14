//! E13 / S07 (v1): phụ đề local — phiên âm bằng whisper.cpp (chạy trên CPU của máy), không gửi tiếng
//! đi đâu (K-S1). Model chỉ được tải khi người dùng đồng ý (biết trước dung lượng), bằng `curl.exe`
//! có sẵn của Windows — app không nhúng thư viện HTTP nào. Kèm đọc / ghi SRT, VTT (X05).

pub mod subtitles;

use mochi_project::{Caption, CaptionWord, Ticks};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Model whisper.cpp (đa ngôn ngữ — có tiếng Việt).
#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: &'static str,
    pub file: &'static str,
    #[specta(type = f64)]
    pub bytes: u64,
}

pub const MODELS: [ModelInfo; 3] = [
    ModelInfo { id: "tiny", file: "ggml-tiny.bin", bytes: 77_691_713 },
    ModelInfo { id: "base", file: "ggml-base.bin", bytes: 147_951_465 },
    ModelInfo { id: "small", file: "ggml-small.bin", bytes: 487_601_967 },
];

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

pub fn model(id: &str) -> Option<ModelInfo> {
    MODELS.iter().copied().find(|m| m.id == id)
}

pub fn model_path(dir: &Path, m: &ModelInfo) -> PathBuf {
    dir.join(m.file)
}

/// Model đã có trên máy (đủ dung lượng, lệch ≤ 1 %).
pub fn is_installed(dir: &Path, m: &ModelInfo) -> bool {
    std::fs::metadata(model_path(dir, m)).is_ok_and(|md| (md.len() as f64 - m.bytes as f64).abs() <= m.bytes as f64 * 0.01)
}

/// whisper.cpp được build với AVX2 / FMA / F16C; CPU thiếu → chạy sẽ chết ngay (SIGILL) → chặn trước.
pub fn cpu_supported() -> Result<(), String> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") && std::is_x86_feature_detected!("f16c") {
            return Ok(());
        }
        Err("CPU thiếu AVX2 / FMA / F16C — không chạy được phiên âm local.".into())
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        Ok(())
    }
}

/// Tải model bằng `curl.exe` của Windows vào `dir` (tệp `.partial` rồi đổi tên). `progress(byte, tổng)`.
/// Hủy giữa chừng → xóa tệp dở. Không có mạng → lỗi rõ ràng, phần còn lại của app vẫn chạy.
pub fn download(dir: &Path, m: &ModelInfo, cancel: Arc<AtomicBool>, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let out = model_path(dir, m);
    let partial = out.with_extension("bin.partial");
    let _ = std::fs::remove_file(&partial);
    let mut child = Command::new("curl.exe")
        .args(["-L", "--fail", "--silent", "--show-error", "--retry", "2", "-o"])
        .arg(&partial)
        .arg(format!("{BASE_URL}{}", m.file))
        .spawn()
        .map_err(|e| format!("không chạy được curl.exe: {e}"))?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&partial);
            return Err("đã hủy".into());
        }
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        progress(std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0), m.bytes);
        std::thread::sleep(Duration::from_millis(250));
    };
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err("không tải được model (máy đang offline hoặc máy chủ không phản hồi)".into());
    }
    std::fs::rename(&partial, &out).map_err(|e| e.to_string())?;
    if !is_installed(dir, m) {
        let _ = std::fs::remove_file(&out);
        return Err("tệp model tải về không đúng dung lượng".into());
    }
    progress(m.bytes, m.bytes);
    Ok(out)
}

/// 48 kHz stereo xen kẽ → 16 kHz mono (lọc thông thấp FIR 7 kHz rồi lấy 1/3 mẫu).
pub fn to_16k_mono(stereo_48k: &[f32]) -> Vec<f32> {
    let mono: Vec<f32> = stereo_48k.chunks_exact(2).map(|p| (p[0] + p[1]) * 0.5).collect();
    const TAPS: usize = 31;
    let fc = 7000.0 / 48000.0;
    let kernel: Vec<f32> = (0..TAPS)
        .map(|i| {
            let n = i as f64 - (TAPS as f64 - 1.0) / 2.0;
            let sinc = if n == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n) };
            let w = 0.54 - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / (TAPS as f64 - 1.0)).cos();
            (sinc * w) as f32
        })
        .collect();
    let sum: f32 = kernel.iter().sum();
    (0..mono.len() / 3)
        .map(|j| {
            let c = j * 3;
            kernel
                .iter()
                .enumerate()
                .map(|(k, w)| {
                    let idx = c as i64 + k as i64 - (TAPS as i64 / 2);
                    if idx < 0 || idx as usize >= mono.len() { 0.0 } else { mono[idx as usize] * w }
                })
                .sum::<f32>()
                / sum
        })
        .collect()
}

/// Phiên âm `pcm` (16 kHz mono) → phụ đề có thời gian từng chữ. `language` = "vi", "en"… hoặc None (tự nhận).
/// `offset` cộng vào mọi mốc (vị trí của đoạn tiếng trên timeline).
pub fn transcribe(
    model_file: &Path,
    pcm: &[f32],
    language: Option<&str>,
    offset: Ticks,
    cancel: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    progress: impl FnMut(i32) + 'static,
) -> Result<Vec<Caption>, String> {
    cpu_supported()?;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    let ctx = WhisperContext::new_with_params(model_file, WhisperContextParameters::default())
        .map_err(|e| format!("không mở được model: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) as i32;
    params.set_n_threads(threads);
    params.set_language(Some(language.unwrap_or("auto")));
    params.set_token_timestamps(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    params.set_progress_callback_safe(progress);
    // whisper gọi hàm này đều đặn trong lúc tính: `hold` bật (đang quay) → đứng chờ tại đây, tức là
    // tạm dừng phiên âm cho tới khi dừng quay, không giành CPU với bản quay. Trả true = hủy.
    // Không dùng `set_abort_callback_safe` của whisper-rs 0.16: nó cất closure dạng
    // `Box<Box<dyn FnMut>>` nhưng trampoline đọc như closure cụ thể → đọc rác, whisper hủy ngẫu nhiên
    // ("failed to encode", mã −6).
    unsafe extern "C" fn should_abort(data: *mut std::ffi::c_void) -> bool {
        let flags = unsafe { &*(data as *const AbortFlags) };
        while flags.hold.load(Ordering::Relaxed) && !flags.cancel.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        flags.cancel.load(Ordering::Relaxed)
    }
    struct AbortFlags {
        cancel: Arc<AtomicBool>,
        hold: Arc<AtomicBool>,
    }
    // `flags` sống tới hết hàm, lâu hơn lời gọi `state.full` dùng con trỏ của nó.
    let flags = AbortFlags { cancel: cancel.clone(), hold };
    unsafe {
        params.set_abort_callback(Some(should_abort));
        params.set_abort_callback_user_data(&flags as *const AbortFlags as *mut std::ffi::c_void);
    }
    state.full(params, pcm).map_err(|e| if cancel.load(Ordering::Relaxed) { "đã hủy".to_string() } else { format!("phiên âm lỗi: {e}") })?;

    // whisper: mốc thời gian theo đơn vị 10 ms.
    let cs = |v: i64| offset + Ticks::from_secs(v as f64 / 100.0);
    let mut out = Vec::new();
    for seg in state.as_iter() {
        let text = seg.to_str_lossy().map(|s| s.trim().to_string()).unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let mut words: Vec<CaptionWord> = Vec::new();
        for i in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(i) else { continue };
            let piece = tok.to_str_lossy().map(|s| s.to_string()).unwrap_or_default();
            if piece.starts_with("[_") || piece.starts_with("<|") || piece.trim().is_empty() {
                continue;
            }
            let d = tok.token_data();
            // Token bắt đầu bằng khoảng trắng = chữ mới; còn lại nối vào chữ trước (một từ nhiều token).
            if piece.starts_with(' ') || words.is_empty() {
                words.push(CaptionWord { start: cs(d.t0), end: cs(d.t1.max(d.t0)), text: piece.trim().to_string() });
            } else if let Some(w) = words.last_mut() {
                w.text.push_str(&piece);
                w.end = cs(d.t1.max(d.t0));
            }
        }
        out.push(Caption {
            id: format!("cap-{}", out.len() + 1),
            start: cs(seg.start_timestamp()),
            end: cs(seg.end_timestamp().max(seg.start_timestamp() + 1)),
            text,
            words,
        });
    }
    Ok(out)
}
