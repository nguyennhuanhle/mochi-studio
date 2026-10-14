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

/// Mức trung bình (dB) của dải tần quanh `hz` trong một file tiếng (lọc bandpass hẹp + volumedetect).
fn band_db(p: &std::path::Path, hz: u32) -> f64 {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(p)
        .args(["-af", &format!("bandpass=f={hz}:width_type=q:w=8,volumedetect"), "-f", "null", "-"])
        .output();
    let Ok(out) = out else { return f64::NAN };
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find_map(|l| l.split("mean_volume:").nth(1).map(|s| s.trim().trim_end_matches(" dB").parse().unwrap_or(f64::NAN)))
        .unwrap_or(f64::NAN)
}

/// R10 có phát tiếng (người dùng đã cho phép): app A (có cửa sổ) phát 440 Hz, app B (không cửa sổ)
/// phát 1000 Hz, cả hai nhỏ tiếng. Quay cửa sổ A hai lần — chỉ tiếng app / toàn hệ thống — rồi đo
/// từng tần số trong luồng tiếng thu được: chỉ tiếng app → có 440, không có 1000.
pub async fn run_audible() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase6").join("app-audio-audible");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let tone = |hz: u32| -> anyhow::Result<std::path::PathBuf> {
        let p = root.join(format!("tone{hz}.wav"));
        let st = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("sine=frequency={hz}:sample_rate=48000:duration=4"), "-af", "volume=0.15"])
            .arg(&p)
            .status()?;
        anyhow::ensure!(st.success());
        Ok(p)
    };
    let (a_wav, b_wav) = (tone(440)?, tone(1000)?);
    let title = "Mochi audio test A";
    let script_a = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $sp = New-Object System.Media.SoundPlayer '{}'; $sp.PlayLooping(); \
         $f = New-Object System.Windows.Forms.Form; $f.Text = '{title}'; $f.TopMost = $true; $f.StartPosition = 'Manual'; \
         $f.Location = New-Object System.Drawing.Point(0, 0); $f.Size = New-Object System.Drawing.Size(240, 160); \
         [System.Windows.Forms.Application]::Run($f)",
        a_wav.display()
    );
    let script_b = format!("$sp = New-Object System.Media.SoundPlayer '{}'; $sp.PlayLooping(); Start-Sleep -Seconds 40", b_wav.display());
    let mut a = Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script_a]).spawn()?;
    let mut b = Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script_b]).spawn()?;
    let title_w: Vec<u16> = title.encode_utf16().chain([0]).collect();
    let mut hwnd = 0isize;
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(h) = unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(None, windows::core::PCWSTR(title_w.as_ptr())) }
            && !h.0.is_null()
        {
            hwnd = h.0 as isize;
            break;
        }
    }
    let result = async {
        anyhow::ensure!(hwnd != 0, "không thấy cửa sổ app A");
        tokio::time::sleep(Duration::from_millis(1000)).await;
        for app_only in [true, false] {
            let service = RecorderService::new();
            let mut events = service.subscribe();
            let config = RecordingConfig {
                target: CaptureTarget::Window { id: (hwnd as u64).to_string() },
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
                app_audio_only: app_only,
            };
            let paths = service.start(config, format!("Audible-{app_only}")).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
            tokio::time::sleep(Duration::from_millis(3000)).await;
            service.stop().await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let mut warnings = vec![];
            while let Ok(e) = events.try_recv() {
                if let RecorderEvent::Warning { warning } = e {
                    warnings.push(format!("{warning:?}"));
                }
            }
            let audio = walk(&paths.recording_dir)
                .into_iter()
                .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("system_audio")))
                .ok_or_else(|| anyhow::anyhow!("không có luồng tiếng"))?;
            println!(
                "  {}: 440 Hz (app A) {:.1} dB, 1000 Hz (app B) {:.1} dB, cảnh báo {:?}",
                if app_only { "chỉ tiếng app A " } else { "toàn hệ thống   " },
                band_db(&audio, 440),
                band_db(&audio, 1000),
                warnings
            );
        }
        Ok(())
    }
    .await;
    let _ = a.kill();
    let _ = b.kill();
    let _ = std::fs::remove_dir_all(&root);
    println!("  đã tắt hai app phát tiếng, xóa bản quay thử");
    result
}
