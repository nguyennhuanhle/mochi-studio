//! Kịch bản kiểm thử thật cho Vòng 2 (Người biên tập — phần dữ liệu): gọi đúng
//! `EditorSession` mà app dùng và IN RA kết quả để người xem tự đánh giá.

use mochi_editor::{EditorError, EditorSession};
use mochi_project::{Project, Ticks};
use mochi_timeline::{Edge, EditOp};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn root() -> PathBuf {
    std::env::current_dir().unwrap().join("recordings").join("round2")
}

fn secs(t: Ticks) -> String {
    format!("{:.2}s", t.secs())
}

fn print_timeline(label: &str, p: &Project) {
    println!("  ── {label}");
    for (i, t) in p.tracks.iter().enumerate() {
        let mut clips: Vec<_> = p.clips.iter().filter(|c| c.track_id == t.id).collect();
        clips.sort_by_key(|c| c.start);
        let desc: Vec<String> = clips
            .iter()
            .map(|c| {
                let name = c
                    .media_id
                    .as_ref()
                    .and_then(|m| p.media.iter().find(|x| &x.id == m))
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                format!(
                    "[{}→{} «{}» src {}–{} x{}]",
                    secs(c.start),
                    secs(c.end()),
                    name.chars().take(18).collect::<String>(),
                    secs(c.source_in),
                    secs(c.source_out),
                    c.speed
                )
            })
            .collect();
        println!(
            "    #{i} {:<8}{}{} {}",
            t.name,
            if t.locked { " 🔒" } else { "" },
            if t.hidden { " (ẩn)" } else { "" },
            if desc.is_empty() { "(trống)".into() } else { desc.join(" ") }
        );
    }
}

fn err(e: &EditorError) -> String {
    serde_json::to_string(e).unwrap()
}

fn ffmpeg(args: &[&str]) {
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "ffmpeg {args:?} thất bại");
}

fn make_media(dir: &Path) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    let v = dir.join("clip-testsrc.mp4");
    let a = dir.join("nhac-nen.mp3");
    let i = dir.join("logo.png");
    ffmpeg(&[
        "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=6",
        "-f", "lavfi", "-i", "sine=frequency=440:duration=6",
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
        v.to_str().unwrap(),
    ]);
    ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=660:duration=4", a.to_str().unwrap()]);
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0xE2557B:size=400x400", "-frames:v", "1", i.to_str().unwrap()]);
    let broken = dir.join("hong.mp4");
    std::fs::write(&broken, b"day khong phai video").unwrap();
    let txt = dir.join("ghi-chu.txt");
    std::fs::write(&txt, "hello").unwrap();
    vec![v, a, i, broken, txt]
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name();
        if name == ".lock" || name == "project.json" || name == "backups" {
            continue;
        }
        if p.is_dir() {
            copy_dir(&p, &to.join(&name));
        } else {
            std::fs::copy(&p, to.join(&name)).unwrap();
        }
    }
}

pub fn run() -> anyhow::Result<()> {
    let root = root();
    let _ = std::fs::remove_dir_all(&root);
    let src = std::env::current_dir()?
        .join("recordings")
        .join("round1")
        .join("Kịch bản basic.mochi");
    anyhow::ensure!(src.is_dir(), "cần chạy `mochi-cli scenario basic` trước (Vòng 1)");
    let dir = root.join("Demo.mochi");
    copy_dir(&src, &dir);
    let files = make_media(&root.join("media"));

    println!("== Mở gói vừa quay (chưa có project.json) → tự tạo project ==");
    let mut s = EditorSession::open(&dir).map_err(|e| anyhow::anyhow!(err(&e)))?;
    let p = s.project().clone();
    println!(
        "  canvas {}×{} @{}fps, {} media, {} clip, project.json tồn tại: {}",
        p.canvas.width,
        p.canvas.height,
        p.canvas.fps,
        p.media.len(),
        p.clips.len(),
        dir.join("project.json").is_file()
    );
    print_timeline("ban đầu", &p);

    println!("\n== E01 Import 5 file + 1 file trùng ==");
    let mut paths = files.clone();
    paths.push(files[0].clone());
    let r = s.import(&paths);
    println!("  imported: {}", r.imported.len());
    for x in &r.rejected {
        println!("  rejected: {} — {}", x.path.file_name().unwrap().to_string_lossy(), x.reason);
    }
    for d in &r.duplicates {
        println!("  duplicate: {} (trùng media đã có)", d.path.file_name().unwrap().to_string_lossy());
    }
    for m in &s.project().media {
        println!(
            "  media: {:<22} {:?} {} {}",
            m.name.chars().take(22).collect::<String>(),
            m.kind,
            secs(m.duration),
            m.video.map(|v| format!("{}×{} {:.0}fps", v.width, v.height, v.fps)).unwrap_or_default()
        );
    }

    let media_id = |s: &EditorSession, name: &str| {
        s.project().media.iter().find(|m| m.name == name).unwrap().id.clone()
    };
    let rec_clip = s.project().clips[0].clone();
    let v1 = s.project().tracks[0].id.clone();
    let end = rec_clip.end();

    println!("\n== E02 Thả media lên timeline ==");
    s.apply(&EditOp::AddClip { media_id: media_id(&s, "clip-testsrc.mp4"), track_id: None, start: end })?;
    s.apply(&EditOp::AddClip { media_id: media_id(&s, "nhac-nen.mp3"), track_id: None, start: Ticks::from_secs(1.0) })?;
    print_timeline("sau khi thả video (tự vào Video 1) + nhạc (Audio 1)", s.project());
    let r = s.apply(&EditOp::AddClip {
        media_id: media_id(&s, "logo.png"),
        track_id: Some(v1.clone()),
        start: Ticks::from_secs(2.0),
    });
    println!("  K-E5 thả ảnh chồng lên clip bản quay ở Video 1 → {}", r.err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    let r = s.apply(&EditOp::AddClip {
        media_id: media_id(&s, "nhac-nen.mp3"),
        track_id: Some(v1.clone()),
        start: Ticks::from_secs(30.0),
    });
    println!("  thả nhạc vào track hình → {}", r.err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    s.apply(&EditOp::AddClip { media_id: media_id(&s, "logo.png"), track_id: None, start: Ticks::from_secs(2.0) })?;
    print_timeline("thả ảnh không chỉ định track → tự tạo Video 2", s.project());

    println!("\n== E02 Cắt / trim / di chuyển / tốc độ ==");
    s.apply(&EditOp::SplitClip { clip_id: rec_clip.id.clone(), at: Ticks::from_secs(3.0) })?;
    let right = s.project().clips.iter().find(|c| c.track_id == v1 && c.start == Ticks::from_secs(3.0)).unwrap().id.clone();
    s.apply(&EditOp::TrimClip { clip_id: rec_clip.id.clone(), edge: Edge::End, to: Ticks::from_secs(2.5) })?;
    print_timeline("cắt bản quay ở 3s, trim nửa trái về 2.5s", s.project());
    let r = s.apply(&EditOp::TrimClip { clip_id: rec_clip.id.clone(), edge: Edge::End, to: Ticks::from_secs(99.0) });
    println!("  trim vượt độ dài media → {}", r.map(|_| "OK (đã kẹp)".to_string()).unwrap_or_else(|e| err(&e)));
    print_timeline("trim tới 99s → kẹp sát mép clip bên phải (3.00s)", s.project());
    let r = s.apply(&EditOp::MoveClips { clip_ids: vec![right.clone()], delta: Ticks::from_secs(-10.0), track_id: None });
    println!("  dời clip về trước 0 → {}", r.err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    s.apply(&EditOp::SetClipSpeed { clip_id: right.clone(), speed: 10.0 })?;
    let sp = s.project().clips.iter().find(|c| c.id == right).unwrap().speed;
    println!("  đặt tốc độ 10x → kẹp còn {sp}x (E04: 0.25–4)");
    print_timeline("sau khi đổi tốc độ", s.project());

    println!("\n== E02 Ripple delete ==");
    s.apply(&EditOp::DeleteClips { clip_ids: vec![rec_clip.id.clone()], ripple: true })?;
    print_timeline("xóa nửa trái (0–3s) có ripple → các clip sau trên Video 1 dồn sang trái đúng 3s", s.project());

    println!("\n== Track khóa ==");
    s.apply(&EditOp::SetTrack { track_id: v1.clone(), name: None, locked: Some(true), muted: None, hidden: None, volume_db: None })?;
    let r = s.apply(&EditOp::SplitClip { clip_id: right.clone(), at: Ticks::from_secs(1.0) });
    println!("  cắt clip trên track khóa → {}", r.err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    s.apply(&EditOp::SetTrack { track_id: v1.clone(), name: None, locked: Some(false), muted: None, hidden: None, volume_db: None })?;

    println!("\n== Undo / Redo ==");
    s.apply(&EditOp::DeleteClips { clip_ids: vec![right.clone()], ripple: false })?;
    let n0 = s.project().clips.len();
    s.undo()?;
    let n1 = s.project().clips.len();
    s.redo()?;
    let n2 = s.project().clips.len();
    s.undo()?;
    println!(
        "  xóa 1 clip: còn {n0} → undo → {n1} → redo → {n2} → undo → {}",
        s.project().clips.len()
    );

    println!("\n== K-E4 Xóa media đang dùng ==");
    let vid = media_id(&s, "clip-testsrc.mp4");
    println!("  không xác nhận → {}", s.remove_media(&vid, false).err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    let v = s.remove_media(&vid, true)?;
    println!("  có xác nhận → còn {} media, {} clip", v.project.media.len(), v.project.clips.len());

    println!("\n== S01 Lưu + backup ==");
    s.save(true)?;
    s.apply(&EditOp::SetTrack { track_id: v1.clone(), name: Some("Màn hình".into()), locked: None, muted: None, hidden: None, volume_db: None })?;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    s.save(true)?;
    for b in s.backups() {
        println!("  backup: {} ({})", b.path.file_name().unwrap().to_string_lossy(), b.saved_at);
    }

    println!("\n== K-E3 Mở cùng project từ process thứ hai (đang giữ khóa) ==");
    let out = Command::new(std::env::current_exe()?)
        .args(["scenario", "editor-open", dir.to_str().unwrap()])
        .output()?;
    println!("  process 2: {}", String::from_utf8_lossy(&out.stdout).trim());
    drop(s);
    let out = Command::new(std::env::current_exe()?)
        .args(["scenario", "editor-open", dir.to_str().unwrap()])
        .output()?;
    println!("  sau khi process 1 đóng project: {}", String::from_utf8_lossy(&out.stdout).trim());

    println!("\n== Media bị thiếu → relink theo thư mục ==");
    let audio = root.join("media").join("nhac-nen.mp3");
    let moved = root.join("chuyen-di").join("nhac-nen.mp3");
    std::fs::create_dir_all(moved.parent().unwrap())?;
    std::fs::rename(&audio, &moved)?;
    let mut s = EditorSession::open(&dir).map_err(|e| anyhow::anyhow!(err(&e)))?;
    println!("  mở lại: thiếu {} media", s.view().missing.len());
    let r = s.relink_folder(&root.join("chuyen-di"));
    println!("  relink thư mục 'chuyen-di' → sửa {}, còn thiếu {}", r.fixed.len(), r.still_missing.len());
    s.save(false)?;
    drop(s);

    println!("\n== project.json hỏng → đề nghị mở backup ==");
    std::fs::write(dir.join("project.json"), b"{ hong roi")?;
    match EditorSession::open(&dir) {
        Err(e) => {
            println!("  mở → {}", err(&e).chars().take(160).collect::<String>());
            if let EditorError::Project { error: mochi_project::ProjectError::Corrupt { backups, .. } } = &e {
                let s = EditorSession::open_backup(&dir, &backups[0].path).map_err(|e| anyhow::anyhow!(err(&e)))?;
                println!("  mở backup mới nhất → {} clip, project.json đã được ghi lại", s.project().clips.len());
            }
        }
        Ok(_) => println!("  mở → OK?!"),
    }

    println!("\n== K-E6 project của phiên bản mới hơn ==");
    let text = std::fs::read_to_string(dir.join("project.json"))?;
    std::fs::write(dir.join("project.json"), text.replacen("\"schemaVersion\": 1", "\"schemaVersion\": 99", 1))?;
    println!("  mở → {}", EditorSession::open(&dir).err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    std::fs::write(dir.join("project.json"), text)?;

    println!("\n== K-E1 ProjectFs từ chối ghi ra ngoài gói / vào recordings ==");
    let pkg = mochi_project::ProjectPackage::acquire(&dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let fs = pkg.fs();
    for target in [
        dir.join("recordings").join("x.txt"),
        dir.join("..").join("ngoai.txt"),
        dir.join("cache").join("ok.txt"),
    ] {
        let r = fs.write(&target, b"x");
        println!(
            "  ghi {} → {}",
            target.strip_prefix(&root).unwrap_or(&target).display(),
            r.map(|_| "cho phép".to_string()).unwrap_or_else(|e| serde_json::to_string(&e).unwrap())
        );
    }
    Ok(())
}

/// Process phụ cho kiểm thử K-E3.
pub fn open_only(dir: &Path) {
    match EditorSession::open(dir) {
        Ok(s) => println!("mở được (recovered = {})", s.view().recovered),
        Err(e) => println!("{}", err(&e)),
    }
}
