//! R02: vạch âm lượng của âm thanh hệ thống trên màn thiết lập quay. Thu loopback của thiết bị phát
//! mặc định (đúng nguồn mà bản quay dùng), chỉ đo mức đỉnh — không ghi, không phát lại.
//! Chạy trên thread riêng (stream WASAPI không chuyển thread được); dừng khi rời màn thiết lập.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
    mpsc,
};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemAudioLevel {
    /// Tên thiết bị phát mặc định (nguồn của "âm thanh hệ thống").
    pub device: String,
    /// Mức đỉnh 0..1 (biên độ) từ lần đọc trước.
    pub peak: f32,
}

struct Meter {
    device: String,
    peak: Arc<AtomicU32>,
    _stop: mpsc::Sender<()>,
}

static METER: Mutex<Option<Meter>> = Mutex::new(None);

fn start() -> Result<Meter, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let device = cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
        .ok_or("không có thiết bị phát")?;
    let peak = Arc::new(AtomicU32::new(0));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let sink = peak.clone();
    std::thread::Builder::new()
        .name("system-audio-meter".into())
        .spawn(move || {
            let capturer = scap_cpal::create_capturer(
                move |data, _, _| {
                    let p = if let Some(s) = data.as_slice::<f32>() {
                        s.iter().fold(0f32, |m, v| m.max(v.abs()))
                    } else if let Some(s) = data.as_slice::<i16>() {
                        s.iter().fold(0f32, |m, v| m.max((*v as f32 / 32768.0).abs()))
                    } else {
                        0.0
                    };
                    sink.fetch_max(p.to_bits(), Ordering::Relaxed);
                },
                |e| tracing::warn!("system audio meter: {e}"),
            );
            let capturer = match capturer.map_err(|e| e.to_string()).and_then(|c| {
                c.play().map_err(|e| e.to_string())?;
                Ok(c)
            }) {
                Ok(c) => c,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            // Chạy tới khi sender bị drop (stop) — giữ stream sống trên thread này.
            let _ = stop_rx.recv();
            drop(capturer);
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(Meter { device, peak, _stop: stop_tx })
}

/// Đọc mức đỉnh từ lần đọc trước (bật thu loopback nếu chưa bật).
pub fn system_audio_level() -> Result<SystemAudioLevel, String> {
    let mut guard = METER.lock().unwrap();
    if guard.is_none() {
        *guard = Some(start()?);
    }
    let m = guard.as_ref().unwrap();
    // Peak lưu dạng bit của f32 không âm → so sánh số nguyên đúng thứ tự.
    let peak = f32::from_bits(m.peak.swap(0, Ordering::Relaxed));
    Ok(SystemAudioLevel { device: m.device.clone(), peak })
}

/// Tắt thu loopback (rời màn thiết lập, hoặc trước khi bắt đầu quay).
pub fn system_audio_meter_stop() {
    METER.lock().unwrap().take();
}
