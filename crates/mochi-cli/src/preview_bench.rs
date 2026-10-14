//! Đo hiệu năng phát của trình phát preview trên media TỔNG HỢP (không dùng bản quay thật).

use mochi_editor::EditorSession;
use mochi_preview::{PlayerCommand, PlayerEvent, PreviewPlayer};
use mochi_project::Ticks;
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf, process::Command, sync::{Arc, Mutex}, time::Duration};

pub async fn run(secs: f64, project: Option<PathBuf>) -> anyhow::Result<()> {
    if let Some(dir) = project {
        let s = EditorSession::open(&dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let paths: HashMap<String, PathBuf> = s
            .project()
            .media
            .iter()
            .filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p)))
            .collect();
        return bench(Arc::new(s.project().clone()), paths, secs, 1.0).await;
    }
    let root = std::env::current_dir()?.join("recordings").join("bench");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let video = root.join("testsrc-1080p60.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=60:duration=12",
               "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "60"])
        .arg(&video)
        .status()?
        .success();
    anyhow::ensure!(ok, "ffmpeg tạo video thử thất bại");
    let logo = root.join("logo.png");
    Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "color=c=0xE2557B:size=300x300", "-frames:v", "1"])
        .arg(&logo)
        .status()?;

    let mut s = EditorSession::create(&root.join("Bench.mochi"), "Bench").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.import(&[video.clone(), logo.clone()]);
    let ids: Vec<String> = s.project().media.iter().map(|m| m.id.clone()).collect();
    s.apply(&EditOp::AddClip { media_id: ids[0].clone(), track_id: None, start: Ticks::ZERO }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.apply(&EditOp::AddClip { media_id: ids[1].clone(), track_id: None, start: Ticks::ZERO }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let logo_clip = s.project().clips.iter().find(|c| c.media_id.as_deref() == Some(ids[1].as_str())).unwrap().id.clone();
    s.apply(&EditOp::SetClipTransform {
        clip_id: logo_clip,
        transform: mochi_project::Transform { x: 0.85, y: 0.2, scale: 0.25, opacity: 0.9, rotation: 0.0 },
    }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let project = Arc::new(s.project().clone());
    let paths: HashMap<String, PathBuf> = project
        .media
        .iter()
        .filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p)))
        .collect();

    bench(project, paths, secs, 9.5).await
}

async fn bench(
    project: Arc<mochi_project::Project>,
    paths: HashMap<String, PathBuf>,
    secs: f64,
    seek_to: f64,
) -> anyhow::Result<()> {
    let gpu = mochi_render::GpuContext::new()?;
    println!("GPU: {}", gpu.adapter_name());
    let log: Arc<Mutex<Vec<PlayerEvent>>> = Arc::default();
    let sink = log.clone();
    let player = PreviewPlayer::spawn(gpu, tokio::runtime::Handle::current(), move |e| sink.lock().unwrap().push(e));
    player.send(PlayerCommand::SetProject { project, paths, proxies: HashMap::new() });
    tokio::time::sleep(Duration::from_millis(12000)).await; // mở decoder + pipeline + frame đầu
    player.send(PlayerCommand::Play);
    tokio::time::sleep(Duration::from_secs_f64(secs)).await;
    player.send(PlayerCommand::Pause);
    tokio::time::sleep(Duration::from_millis(300)).await;
    player.send(PlayerCommand::Seek(Ticks::from_secs(seek_to)));
    tokio::time::sleep(Duration::from_millis(1500)).await;

    for e in log.lock().unwrap().iter() {
        match e {
            PlayerEvent::Stats { fps, render_ms, layers } => {
                println!("  stats: {fps:5.1} fps phát, render {render_ms:6.2} ms/frame, {layers} lớp")
            }
            PlayerEvent::Time { t, playing } if !playing => println!("  dừng tại {:.3}s", t.secs()),
            _ => {}
        }
    }
    let times: Vec<f64> = log.lock().unwrap().iter().filter_map(|e| match e {
        PlayerEvent::Time { t, playing: true } => Some(t.secs()),
        _ => None,
    }).collect();
    if let (Some(a), Some(b)) = (times.first(), times.last()) {
        println!("  đồng hồ phát chạy từ {a:.2}s → {b:.2}s ({} mốc thời gian)", times.len());
    }
    Ok(())
}

type Opened = (Arc<mochi_project::Project>, HashMap<String, PathBuf>);

/// Mở project như trình biên tập (rồi đóng ngay để nhả .lock), trả về project + đường dẫn media.
fn open_project(dir: &std::path::Path) -> anyhow::Result<Opened> {
    let s = EditorSession::open(dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let paths = s
        .project()
        .media
        .iter()
        .filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p)))
        .collect();
    Ok((Arc::new(s.project().clone()), paths))
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// Vòng 4: làm ấm preview lúc dừng quay.
pub async fn warm(dir: PathBuf) -> anyhow::Result<()> {
    use std::time::Instant;
    let gpu = mochi_render::GpuContext::new()?;
    let ms = |t: Instant| format!("{:.0} ms", t.elapsed().as_secs_f64() * 1000.0);

    println!("== Frame đầu khi renderer còn nguội ==");
    let (p, paths) = open_project(&dir)?;
    let mut cold = mochi_render::Renderer::new(gpu.clone());
    let t0 = Instant::now();
    cold.render(&p, &paths, Ticks::from_secs(1.0), true).await;
    println!("  renderer mới, frame t=1s: {}", ms(t0));
    drop(cold);

    println!("
== Làm ấm rồi mở lại project (như lúc bấm 'Mở trong trình biên tập') ==");
    let mut warm = mochi_render::Renderer::new(gpu.clone());
    let t0 = Instant::now();
    warm.render(&p, &paths, Ticks::from_secs(0.0), true).await;
    println!("  làm ấm (frame t=0, chạy nền sau khi dừng quay): {}", ms(t0));
    let (p2, paths2) = open_project(&dir)?;
    let same: Vec<bool> = p.media.iter().map(|m| p2.media.iter().any(|n| n.id == m.id)).collect();
    println!("  id media giữ nguyên sau khi mở lại: {same:?}");
    warm.retain_media(&p2);
    let t0 = Instant::now();
    warm.render(&p2, &paths2, Ticks::from_secs(1.0), true).await;
    println!("  frame đầu trong trình biên tập (t=1s): {}", ms(t0));

    println!("
== Bỏ bản quay sau khi đã làm ấm: trình phát phải thả file thì mới xóa được ==");
    let root = std::env::current_dir()?.join("recordings").join("round4");
    std::fs::create_dir_all(&root)?;
    let copy = root.join("Warm-copy.mochi");
    let _ = std::fs::remove_dir_all(&copy);
    copy_dir(&dir, &copy)?;
    let _ = std::fs::remove_file(copy.join("project.json"));
    let _ = std::fs::remove_file(copy.join(".lock"));
    let (pc, pathsc) = open_project(&copy)?;
    let player = PreviewPlayer::spawn(gpu.clone(), tokio::runtime::Handle::current(), |_| {});
    player.send(PlayerCommand::SetProject { project: pc, paths: pathsc, proxies: HashMap::new() });
    tokio::time::sleep(Duration::from_secs(12)).await;
    let probe = copy.join("recordings");
    let locked = std::fs::read_dir(&probe)?
        .flatten()
        .flat_map(|r| std::fs::read_dir(r.path().join("content/segments")).into_iter().flatten().flatten())
        .flat_map(|seg| std::fs::read_dir(seg.path()).into_iter().flatten().flatten())
        .filter(|f| f.path().extension().is_some_and(|e| e == "mp4"))
        .map(|f| {
            let r = std::fs::OpenOptions::new().write(true).open(f.path()).and_then(|_| {
                let tmp = f.path().with_extension("mp4.ren");
                std::fs::rename(f.path(), &tmp)?;
                std::fs::rename(&tmp, f.path())
            });
            format!("{}: {}", f.file_name().to_string_lossy(), if r.is_ok() { "đổi tên được" } else { "ĐANG BỊ GIỮ" })
        })
        .collect::<Vec<_>>();
    println!("  khi trình phát đang giữ nguồn: {locked:?}");
    let (tx, rx) = std::sync::mpsc::channel();
    let t0 = Instant::now();
    player.send(PlayerCommand::Unload { done: tx });
    let _ = rx.recv_timeout(Duration::from_secs(5));
    println!("  Unload xong sau {}", ms(t0));
    let t0 = Instant::now();
    let r = mochi_recorder::finalize::remove_project_dir(&root, &copy);
    println!("  remove_project_dir → {:?} sau {}; thư mục còn: {}", r.map_err(|e| e.to_string()), ms(t0), copy.exists());
    Ok(())
}
