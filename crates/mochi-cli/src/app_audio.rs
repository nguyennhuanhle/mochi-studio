//! R10 (v1): kiểm thử "chỉ thu tiếng của app đang quay" mà KHÔNG phát tiếng ra loa.
//!
//! Quay cửa sổ thử (tiến trình PowerShell riêng, không phát âm thanh) với `app_audio_only`, có tạm
//! dừng / quay tiếp, rồi in: build Windows, PID của cửa sổ, cảnh báo nhận được, từng segment trong
//! `recording-meta.json` (đường dẫn + start_time của luồng tiếng so với hình), độ dài luồng tiếng so
//! với hình và mức to nhất của luồng tiếng (app không phát gì → phải là lặng hoàn toàn, kể cả khi
//! máy đang phát nhạc ở app khác). Bản quay bị xóa sau khi đo.

use crate::follow_window::{spawn_test_window, walk};
use mochi_recorder::{CaptureTarget, KeystrokeMode, RecorderEvent, RecorderService, RecordingConfig, RecordingQuality};
use std::{process::Command, time::Duration};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
};

fn probe_secs(p: &std::path::Path) -> f64 {
    mochi_project::media::probe(p).map(|m| m.duration.secs()).unwrap_or(-1.0)
}

/// Mức đỉnh (dBFS) của một file tiếng, qua ffmpeg volumedetect.
fn peak_db(p: &std::path::Path) -> String {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(p)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output();
    let Ok(out) = out else { return "?".into() };
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find_map(|l| l.split("max_volume:").nth(1).map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "không có mẫu".into())
}

pub async fn run() -> anyhow::Result<()> {
    let build = mochi_recorder::app_audio::os_build();
    println!(
        "  Windows build {build} — process loopback {}",
        if mochi_recorder::app_audio::supported() { "được hỗ trợ" } else { "KHÔNG hỗ trợ" }
    );
    let root = std::env::current_dir()?.join("recordings").join("phase6").join("app-audio");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let (hwnd, mut child) = spawn_test_window()?;
    let id = (hwnd as u64).to_string();
    println!(
        "  cửa sổ thử HWND {hwnd:#x}, PID chủ {:?} (tiến trình PowerShell con: {})",
        mochi_recorder::app_audio::window_pid(&id),
        child.id()
    );
    tokio::time::sleep(Duration::from_millis(500)).await;

    let service = RecorderService::new();
    let mut events = service.subscribe();
    let config = RecordingConfig {
        target: CaptureTarget::Window { id },
        mic: None,
        camera: None,
        system_audio: true,
        fps: 30,
        quality: RecordingQuality::Standard,
        countdown_secs: 0,
        keystrokes: KeystrokeMode::Off,
        output_root: root.clone(),
        cursor_highlight: Default::default(),
        frame: true,
        app_audio_only: true,
    };
    let paths = service.start(config, "AppAudio".into()).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    service.pause().await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    tokio::time::sleep(Duration::from_millis(700)).await;
    service.resume().await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    let rec = service.stop().await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();

    let mut warnings = vec![];
    while let Ok(e) = events.try_recv() {
        if let RecorderEvent::Warning { warning } = e {
            warnings.push(format!("{warning:?}"));
        }
    }
    println!("  {} segment, cảnh báo: {:?}", rec.segments, warnings);

    let meta_path = walk(&paths.recording_dir)
        .into_iter()
        .find(|p| p.ends_with("recording-meta.json"))
        .ok_or_else(|| anyhow::anyhow!("không thấy recording-meta.json"))?;
    let dir = meta_path.parent().unwrap().to_path_buf();
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
    let segments = meta["segments"].as_array().cloned().unwrap_or_default();
    for (i, s) in segments.iter().enumerate() {
        let disp = s["display"]["path"].as_str().unwrap_or("");
        let d_start = s["display"]["start_time"].as_f64().unwrap_or(0.0);
        let d_len = probe_secs(&dir.join(disp));
        match s.get("system_audio").filter(|v| !v.is_null()) {
            Some(a) => {
                let ap = dir.join(a["path"].as_str().unwrap_or(""));
                let a_start = a["start_time"].as_f64().unwrap_or(0.0);
                println!(
                    "  segment {i}: hình {d_len:.3} s (start {d_start:.3}); tiếng app {} — {:.3} s, lệch start {:+.0} ms, đỉnh {}",
                    a["path"].as_str().unwrap_or(""),
                    probe_secs(&ap),
                    (a_start - d_start) * 1000.0,
                    peak_db(&ap)
                );
            }
            None => println!("  segment {i}: hình {d_len:.3} s — KHÔNG có luồng tiếng hệ thống/app"),
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    println!("  đã xóa bản quay thử");
    Ok(())
}
