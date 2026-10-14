//! Kịch bản kiểm thử thật Vòng 5 (Hệ thống) trên media TỔNG HỢP: proxy + tạm hoãn (S03, K-S4),
//! tốc độ preview gốc vs proxy, proxy lỗi + thử lại, cache (giới hạn, K-S3), autosave (S01), kiểm GPU.

use mochi_editor::EditorSession;
use mochi_jobs::{JobScheduler, JobStatus, ProxyJob, cache};
use mochi_project::Ticks;
use mochi_timeline::EditOp;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn ffmpeg(args: &[&str]) {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "ffmpeg {args:?} lỗi");
}

fn probe(file: &Path) -> String {
    let o = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_name,width,height,r_frame_rate,duration,nb_frames", "-of", "compact=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn keyframe_interval(file: &Path) -> String {
    let o = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-skip_frame", "nokey", "-show_entries", "frame=pts_time", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    let t: Vec<f64> = String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect();
    if t.len() < 2 {
        return format!("{} keyframe", t.len());
    }
    format!("{} keyframe, cách nhau ~{:.2} s", t.len(), (t[t.len() - 1] - t[0]) / (t.len() - 1) as f64)
}

fn wait_for(s: &JobScheduler, key: &str, pred: impl Fn(&JobStatus) -> bool, secs: u64) -> Option<JobStatus> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if let Some(st) = s.status().get(key)
            && pred(st)
        {
            return Some(st.clone());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("round5");
    let media = root.join("media");
    std::fs::create_dir_all(&media)?;
    let src = media.join("testsrc-4k.mp4");
    if !src.is_file() {
        println!("(tạo video thử 4K 20 s…)");
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=size=3840x2160:rate=30:duration=20", "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", src.to_str().unwrap()]);
    }
    let broken = media.join("hong-4k.mp4");
    let bytes = std::fs::read(&src)?;
    std::fs::write(&broken, &bytes[..bytes.len() / 50])?; // cắt cụt: đầu file hợp lệ, phần còn lại mất

    let dir = root.join("Jobs.mochi");
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = EditorSession::create(&dir, "Jobs").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.import(&[src.clone()]);
    let item = s.project().media[0].clone();
    s.apply(&EditOp::AddClip { media_id: item.id.clone(), track_id: None, start: Ticks::ZERO }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("== S03: media cần proxy? ==");
    println!("  {} ({}×{}) → {}", item.name, item.video.unwrap().width, item.video.unwrap().height, mochi_jobs::needs_proxy(&item));

    println!("\n== S03 + K-S4: tạo proxy, tạm hoãn giữa chừng (như lúc bắt đầu quay), chạy tiếp ==");
    let sched = JobScheduler::new();
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let t0 = Instant::now();
    {
        let log = log.clone();
        sched.on_update(move |k, st| {
            let line = match st {
                JobStatus::Running { progress } if (progress * 100.0) as u32 % 25 != 0 => return,
                JobStatus::Running { progress } => format!("chạy {:.0}%", progress * 100.0),
                other => format!("{other:?}"),
            };
            let mut l = log.lock().unwrap();
            let entry = format!("{:>5.1}s {}: {line}", t0.elapsed().as_secs_f64(), &k[..8]);
            if l.last().map(|x| x.split_once(": ").map(|p| p.1)) != Some(Some(&line)) {
                l.push(entry);
            }
        });
    }
    let target = mochi_jobs::proxy_path(&dir, &item.id);
    let job = ProxyJob { key: item.id.clone(), source: src.clone(), target: target.clone() };
    sched.push(job.clone());
    wait_for(&sched, &item.id, |st| matches!(st, JobStatus::Running { progress } if *progress >= 0.3), 120);
    sched.suspend();
    wait_for(&sched, &item.id, |st| matches!(st, JobStatus::Suspended), 30);
    let partial = target.with_extension("partial.mp4");
    println!("  sau suspend(): trạng thái {:?}, file dở còn: {}", sched.status().get(&item.id), partial.exists());
    std::thread::sleep(Duration::from_secs(3));
    println!("  3 s sau vẫn hoãn: {:?}", sched.status().get(&item.id));
    sched.resume();
    let done = wait_for(&sched, &item.id, |st| matches!(st, JobStatus::Ready { .. } | JobStatus::Failed { .. }), 300);
    for l in log.lock().unwrap().iter() {
        println!("    {l}");
    }
    println!("  kết quả: {done:?}");
    println!("  gốc : {}", probe(&src));
    println!("  proxy: {}  · {} · {:.1} MB", probe(&target), keyframe_interval(&target), std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0) as f64 / 1e6);
    println!("  gốc : {} · {:.1} MB", keyframe_interval(&src), bytes.len() as f64 / 1e6);

    println!("\n== Preview: file gốc 4K vs proxy (renderer của preview, canvas 1080p) ==");
    let project = s.project().clone();
    let gpu = mochi_render::GpuContext::new()?;
    for (label, path) in [("gốc 4K", src.clone()), ("proxy", target.clone())] {
        let paths = [(item.id.clone(), path)].into_iter().collect();
        let mut r = mochi_render::Renderer::new(gpu.clone());
        r.render(&project, &paths, Ticks::from_secs(0.0), true).await;
        let t = Instant::now();
        for i in 0..60 {
            r.render(&project, &paths, Ticks::from_secs(1.0 + i as f64 / 30.0), false).await;
        }
        let play = t.elapsed().as_secs_f64() * 1000.0 / 60.0;
        let mut seeks = Vec::new();
        for at in [15.3, 3.1, 9.7, 18.2, 6.4, 12.9] {
            let t = Instant::now();
            r.render(&project, &paths, Ticks::from_secs(at), true).await;
            seeks.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "  {label:<7}: phát {play:.1} ms/frame · tua {:?} ms (trung bình {:.0} ms)",
            seeks.iter().map(|v| v.round() as i64).collect::<Vec<_>>(),
            seeks.iter().sum::<f64>() / seeks.len() as f64
        );
    }

    println!("\n== Proxy lỗi (file hỏng) → đánh dấu lỗi, thử lại ==");
    let bad = ProxyJob { key: "hong-media-0000".into(), source: broken.clone(), target: mochi_jobs::proxy_path(&dir, "hong-media-0000") };
    sched.push(bad.clone());
    let st = wait_for(&sched, &bad.key, |st| matches!(st, JobStatus::Ready { .. } | JobStatus::Failed { .. }), 120);
    println!("  lần 1: {st:?}");
    sched.retry(bad.clone());
    let st = wait_for(&sched, &bad.key, |st| matches!(st, JobStatus::Ready { .. } | JobStatus::Failed { .. }), 120);
    println!("  thử lại: {st:?}");
    let ok_after = sched.status().get(&item.id).cloned();
    println!("  việc khác không bị chặn: proxy 4K vẫn {:?}", ok_after.map(|s| matches!(s, JobStatus::Ready { .. })));

    println!("\n== Cache (K-S3) ==");
    s.save(true).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    std::fs::create_dir_all(cache::cache_root(&dir).join("thumbs"))?;
    std::fs::write(cache::cache_root(&dir).join("thumbs").join("x.jpg"), vec![0u8; 50_000])?;
    let dirs = vec![dir.clone()];
    let u = cache::usage(&dirs);
    println!("  dung lượng: tổng {:.1} MB (proxy {:.1} MB, ảnh {:.2} MB)", u.total_bytes as f64 / 1e6, u.proxies_bytes as f64 / 1e6, u.thumbs_bytes as f64 / 1e6);
    let rec_file = src.clone();
    for (label, p) in [
        ("media gốc (ngoài gói)", rec_file.clone()),
        ("project.json", dir.join("project.json")),
        ("cache/../project.json", cache::cache_root(&dir).join("..").join("project.json")),
        ("chính thư mục cache/", cache::cache_root(&dir)),
    ] {
        println!("  safe_remove({label}) → {:?}", cache::safe_remove(&dir, &p).err().map(|e| e.to_string()));
    }
    let removed = cache::enforce_limit(&dirs, u.total_bytes - 1);
    println!(
        "  giới hạn = tổng − 1 byte → xóa: {:?}",
        removed.iter().map(|e| format!("{}/{}", e.kind, e.path.file_name().unwrap().to_string_lossy())).collect::<Vec<_>>()
    );
    let freed = cache::clear(&dirs, &[]);
    println!("  dọn hết: giải phóng {:.2} MB; còn lại {} byte", freed as f64 / 1e6, cache::usage(&dirs).total_bytes);
    println!("  sau khi dọn: media gốc còn {}, project.json còn {}", src.is_file(), dir.join("project.json").is_file());

    println!("\n== S01: tự lưu sau thay đổi lớn + lỗi tự lưu thì thử lại sau 10 s ==");
    s.apply(&EditOp::AddTrack { kind: mochi_project::TrackKind::Video }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("  thay đổi nhỏ (thêm track), ngay sau đó: autosave_if_due → {:?}", s.autosave_if_due().map(|r| r.is_ok()));
    let clip = s.project().clips[0].id.clone();
    s.apply(&EditOp::SplitClip { clip_id: clip, at: Ticks::from_secs(5.0) }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("  thay đổi lớn (cắt clip), ngay sau đó: {:?}", s.autosave_if_due().map(|r| r.is_ok()));
    std::thread::sleep(Duration::from_millis(2100));
    println!("  2,1 s sau: {:?} (đã lưu)", s.autosave_if_due().map(|r| r.is_ok()));
    let pj = dir.join("project.json");
    let mut perm = std::fs::metadata(&pj)?.permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&pj, perm.clone())?;
    s.apply(&EditOp::DeleteClips { clip_ids: vec![s.project().clips[0].id.clone()], ripple: false }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    std::thread::sleep(Duration::from_millis(2100));
    let t0 = Instant::now();
    let r = s.autosave_if_due();
    println!("  project.json bị khóa (read-only) → thử lưu: {:?}", r.map(|r| r.err().map(|e| e.to_string())));
    let mut attempts = vec![];
    while t0.elapsed() < Duration::from_secs(23) {
        std::thread::sleep(Duration::from_millis(500));
        if t0.elapsed() > Duration::from_secs(14) && perm.readonly() {
            perm.set_readonly(false);
            std::fs::set_permissions(&pj, perm.clone())?;
            attempts.push(format!("{:.1}s: mở khóa file", t0.elapsed().as_secs_f64()));
        }
        if let Some(r) = s.autosave_if_due() {
            attempts.push(format!("{:.1}s: thử lại → {}", t0.elapsed().as_secs_f64(), if r.is_ok() { "OK" } else { "lỗi" }));
        }
    }
    println!("  {}", attempts.join(" · "));
    println!("  còn thay đổi chưa lưu: {}", s.is_dirty());
    drop(s);

    println!("\n== Kiểm GPU lúc khởi động ==");
    println!("  máy này: {:?}", mochi_render::GpuContext::probe());
    unsafe { std::env::set_var("MOCHI_TEST_FORCE_SOFTWARE_GPU", "1") };
    println!("  ép GPU phần mềm: {:?}", mochi_render::GpuContext::probe());
    unsafe { std::env::remove_var("MOCHI_TEST_FORCE_SOFTWARE_GPU") };
    let _ = PathBuf::new();
    Ok(())
}
