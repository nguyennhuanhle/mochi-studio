//! Phase 4 (R06 + R07): quay bằng đúng `RecorderService` của app một vùng 64×64 ở góc màn hình
//! + webcam + mic, chất lượng "Tiêu chuẩn", 60 fps. Người dùng rút webcam / mic USB giữa chừng.
//! In cảnh báo nhận được, trạng thái bản quay và thông số từng luồng (độ dài, kích thước, fps).
//! Chỉ đọc số liệu bằng ffprobe — không mở hình, không nghe tiếng. Xong thì xóa bản quay.

use mochi_recorder::{
    CaptureTarget, KeystrokeMode, RecorderEvent, RecorderService, RecordingConfig, RecordingQuality, targets,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Mỗi luồng: (loại, rộng×cao, r_frame_rate, độ dài giây).
fn streams(file: &Path) -> Vec<String> {
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,r_frame_rate,duration,nb_frames",
            "-of",
            "compact=p=0:nk=0",
        ])
        .arg(file)
        .output();
    let Ok(o) = o else { return vec![] };
    String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect()
}

fn media_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            // Thư mục fragment (display/, camera/): chỉ lấy init + segment đầu là đủ biết thông số.
            media_files(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| ["mp4", "m4a", "ogg"].contains(&x.to_str().unwrap_or("")))
        {
            out.push(p);
        }
    }
}

pub async fn run(secs: u64, camera: &str, mic: &str, pause_at: Option<u64>, keep: bool) -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase4");
    std::fs::create_dir_all(&root)?;
    let devices = targets::list_devices();
    let cam = devices
        .cameras
        .iter()
        .find(|c| c.name.contains(camera))
        .ok_or_else(|| anyhow::anyhow!("không thấy webcam chứa \"{camera}\"; có: {:?}", devices.cameras.iter().map(|c| &c.name).collect::<Vec<_>>()))?;
    let mic_name = devices
        .mics
        .iter()
        .find(|m| m.contains(mic))
        .ok_or_else(|| anyhow::anyhow!("không thấy mic chứa \"{mic}\"; có: {:?}", devices.mics))?
        .clone();
    let display = targets::list_displays().into_iter().next().expect("không có màn hình");
    let config = RecordingConfig {
        target: CaptureTarget::Area { display_id: display.id, x: 0.0, y: 0.0, width: 64.0, height: 64.0 },
        mic: Some(mic_name.clone()),
        camera: Some(cam.id.clone()),
        system_audio: false,
        fps: 60,
        quality: RecordingQuality::Standard,
        countdown_secs: 0,
        keystrokes: KeystrokeMode::Off,
        output_root: root.clone(),
        cursor_highlight: Default::default(),
        frame: true, app_audio_only: false,
    };
    let service = RecorderService::new();
    let mut events = service.subscribe();
    let t0 = Instant::now();
    tokio::spawn(async move {
        while let Ok(e) = events.recv().await {
            match e {
                RecorderEvent::Stats { .. } => {}
                other => println!("  [{:>5.1}s] sự kiện: {}", t0.elapsed().as_secs_f64(), serde_json::to_string(&other).unwrap_or_default()),
            }
        }
    });

    println!("== Quay {secs} s: vùng 64×64 + webcam \"{}\" + mic \"{mic_name}\", Tiêu chuẩn, 60 fps ==", cam.name);
    let paths = service.start(config, "Thu rut thiet bi".into()).await?;
    println!("  >>> ĐANG QUAY — rút webcam / mic USB bất kỳ lúc nào trong {secs} s <<<");
    let mut paused = false;
    while t0.elapsed().as_secs() < secs {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Some(p) = pause_at
            && !paused
            && t0.elapsed().as_secs() >= p
        {
            paused = true;
            println!("  [{:>5.1}s] tạm dừng: {:?}", t0.elapsed().as_secs_f64(), service.pause().await);
            tokio::time::sleep(Duration::from_secs(2)).await;
            println!("  [{:>5.1}s] tiếp tục: {:?}", t0.elapsed().as_secs_f64(), service.resume().await);
        }
    }
    let stop = service.stop().await;
    println!("  [{:>5.1}s] dừng: {}", t0.elapsed().as_secs_f64(), match &stop {
        Ok(f) => format!("OK, {:.1} s, {} segment", f.duration_ms / 1000.0, f.segments),
        Err(e) => format!("LỖI {e}"),
    });

    println!("\n== Bản quay ==");
    match cap_project::RecordingMeta::load_for_project(&paths.recording_dir) {
        Ok(meta) => {
            if let Some(cap_project::StudioRecordingMeta::MultipleSegments { inner }) = meta.studio_meta() {
                println!("  trạng thái: {:?}", inner.status);
                for (i, s) in inner.segments.iter().enumerate() {
                    println!(
                        "  segment {i}: màn hình={} · webcam={} · mic={}",
                        s.display.path.as_str(),
                        s.camera.as_ref().map_or("—".into(), |c| c.path.as_str().to_string()),
                        s.mic.as_ref().map_or("—".into(), |m| m.path.as_str().to_string()),
                    );
                }
            }
        }
        Err(e) => println!("  không đọc được recording-meta: {e}"),
    }
    if let Ok(d) = std::fs::read_to_string(paths.recording_dir.join("recording-diagnostics.json")) {
        println!("  recording-diagnostics.json: {}", d.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let mut files = Vec::new();
    media_files(&paths.recording_dir, &mut files);
    files.sort();
    for f in files.iter().filter(|f| {
        let n = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !n.starts_with("segment_") || n.ends_with("_000.m4s") || n == "init.mp4"
    }) {
        let name = f.strip_prefix(&paths.recording_dir).unwrap_or(f).display().to_string();
        for s in streams(f) {
            println!("  {name}: {s}");
        }
    }

    if keep {
        println!("\n  giữ bản quay ở {}", paths.project_dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&paths.project_dir);
        println!("\n  đã xóa bản quay (riêng tư)");
    }
    Ok(())
}
