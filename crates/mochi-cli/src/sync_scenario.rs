//! S05 với bản quay dài (Vòng 5): quay bằng đúng `RecorderService` của app một vùng 64×64 ở góc
//! màn hình + âm thanh hệ thống (KHÔNG mic), rồi đo độ dài từng luồng, tạo project, xuất MP4 và đo
//! lại. Chỉ đo thời lượng — không mở hình, không nghe tiếng. Xong thì xóa bản quay và file xuất.

use crate::export_cmd;
use mochi_recorder::{CaptureTarget, KeystrokeMode, RecorderService, RecordingConfig, RecordingQuality, targets};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Thời lượng (giây) của từng luồng trong file: (loại, giây).
fn stream_durations(file: &Path) -> Vec<(String, f64)> {
    let o = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,duration:format=duration", "-of", "csv=p=0"])
        .arg(file)
        .output();
    let Ok(o) = o else { return vec![] };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split(',');
            let a = it.next()?.trim().to_string();
            match it.next() {
                Some(d) => Some((a, d.trim().parse().ok()?)),
                None => Some(("format".into(), a.parse().ok()?)),
            }
        })
        .collect()
}

fn media_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            media_files(&p, out);
        } else if p.extension().is_some_and(|x| ["mp4", "m4a", "ogg", "aac", "wav"].contains(&x.to_str().unwrap_or(""))) {
            out.push(p);
        }
    }
}

pub async fn run(secs: u64, keep: bool) -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("round5");
    std::fs::create_dir_all(&root)?;
    let display = targets::list_displays().into_iter().next().expect("không có màn hình");
    let config = RecordingConfig {
        target: CaptureTarget::Area { display_id: display.id, x: 0.0, y: 0.0, width: 64.0, height: 64.0 },
        mic: None,
        camera: None,
        system_audio: true,
        fps: 30,
        quality: RecordingQuality::Standard,
        countdown_secs: 0,
        keystrokes: KeystrokeMode::Off,
        output_root: root.clone(), cursor_highlight: Default::default(), frame: true,
    };
    let service = RecorderService::new();
    println!("== S05: quay {secs} s (vùng 64×64 góc trên-trái, âm thanh hệ thống, không mic) ==");
    let started = Instant::now();
    let paths = service.start(config, "Do dong bo 60 phut".into()).await?;
    let mut next = 300u64.min(secs);
    while started.elapsed().as_secs() < secs {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if started.elapsed().as_secs() >= next {
            println!("  đang quay: {} phút", next / 60);
            next += 300;
        }
    }
    let finished = service.stop().await?;
    let wall = started.elapsed().as_secs_f64();
    println!(
        "  dừng: đồng hồ thật {:.3} s, RecorderService báo {:.3} s, {} segment",
        wall,
        finished.duration_ms / 1000.0,
        finished.segments
    );

    println!("\n== Độ dài từng luồng trong bản quay ==");
    let mut files = Vec::new();
    media_files(&paths.recording_dir, &mut files);
    files.sort();
    let (mut video, mut audio) = (0.0, 0.0);
    for f in &files {
        let d = stream_durations(f);
        let secs: f64 = d.iter().find(|(k, _)| k != "format").map(|x| x.1).unwrap_or(0.0);
        let name = f.strip_prefix(&paths.recording_dir).unwrap_or(f).display().to_string();
        if name.contains("display") {
            video += secs;
        } else if name.contains("audio") {
            audio += secs;
        }
        println!("  {name}: {secs:.3} s");
    }
    // Tiếng bắt đầu lệch hình một đoạn (start_time trong recording-meta) — đó là độ dời cố định,
    // đã được căn khi phát / xuất. Trôi đồng hồ = phần lệch còn lại sau khi trừ độ dời đó.
    let mut shift = 0.0;
    if let Ok(meta) = cap_project::RecordingMeta::load_for_project(&paths.recording_dir)
        && let Some(cap_project::StudioRecordingMeta::MultipleSegments { inner }) = meta.studio_meta()
    {
        for seg in &inner.segments {
            if let Some(a) = &seg.system_audio {
                let d = seg.display.start_time.unwrap_or(0.0);
                shift = a.start_time.unwrap_or(0.0) - d;
                println!("  start_time: hình {d:.3}, tiếng {:.3} → tiếng bắt đầu {:+.0} ms so với hình", a.start_time.unwrap_or(0.0), shift * 1000.0);
            }
        }
    }
    let covered = video - shift;
    println!(
        "  tổng hình {video:.3} s, tiếng {audio:.3} s; phần hình mà tiếng phủ {covered:.3} s → trôi {:+.0} ms; hệ số bù S05 = ×{:.5}",
        (audio - covered) * 1000.0,
        mochi_preview::drift_rate(audio, covered)
    );

    println!("\n== Xuất MP4 360p cả bản quay, rồi đo ==");
    let (project, media_paths) = export_cmd::load(&paths.project_dir)?;
    let out = root.join("sync-export.mp4");
    let mut s = export_cmd::settings(&out);
    s.short_side = 360;
    s.overwrite = true;
    let r = export_cmd::export(&project, &media_paths, &s, &[], |_| false);
    match &r.result {
        Ok(m) => println!("  {}", serde_json::to_string(m)?),
        Err(e) => println!("  LỖI {}", export_cmd::err(e)),
    }
    let d = stream_durations(&out);
    let v = d.iter().find(|(k, _)| k == "video").map(|x| x.1).unwrap_or(0.0);
    let a = d.iter().find(|(k, _)| k == "audio").map(|x| x.1).unwrap_or(0.0);
    println!("  file xuất: hình {v:.3} s, tiếng {a:.3} s → lệch {:+.0} ms (mục tiêu < 100 ms)", (a - v) * 1000.0);

    if !keep {
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_dir_all(&paths.project_dir);
        println!("\n  đã xóa bản quay và file xuất (riêng tư)");
    }
    Ok(())
}
