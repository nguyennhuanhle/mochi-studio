//! Phase 4 lượt 2, đợt B (Người biên tập): gọi hàm thật trên media TỔNG HỢP và in kết quả.
//! E11 âm lượng track · kẹp giá trị · backup (tự lưu thưa, bản lưu tay) · crash → khôi phục có lựa chọn ·
//! project hỏng → chọn backup cũ hơn · relink theo tên / relink bản quay · GIF động · proxy bản quay 4K ·
//! webcam đặt tự do.

use crate::fake_recording;
use mochi_editor::EditorSession;
use mochi_project::{ClipContent, NormPoint, Ticks, TrackKind};
use mochi_timeline::EditOp;
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

fn ffmpeg(args: &[&str]) -> anyhow::Result<()> {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status()?.success();
    anyhow::ensure!(ok, "ffmpeg {args:?}");
    Ok(())
}

fn e(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

fn paths(s: &EditorSession) -> HashMap<String, PathBuf> {
    s.project().media.iter().filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p))).collect()
}

fn arrow() -> anyhow::Result<PathBuf> {
    std::fs::read_dir("recordings/round1")?
        .flatten()
        .flat_map(|p| std::fs::read_dir(p.path().join("recordings")).into_iter().flatten().flatten())
        .map(|r| r.path().join("content/cursors/cursor_0.png"))
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow::anyhow!("cần ảnh con trỏ từ một bản quay Vòng 1"))
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase4").join("editor");
    let _ = std::fs::remove_dir_all(&root);
    let media = root.join("media");
    std::fs::create_dir_all(&media)?;
    let tone = media.join("tone.wav");
    ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-ac", "2", "-ar", "48000", tone.to_str().unwrap()])?;
    let gif = media.join("anim.gif");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc=size=320x180:rate=10:duration=2", gif.to_str().unwrap()])?;
    let clipv = media.join("clip.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30:duration=2", "-pix_fmt", "yuv420p", clipv.to_str().unwrap()])?;

    // ---------------- E11 âm lượng track ----------------
    println!("== E11 âm lượng theo track ==");
    let dir = root.join("Audio.mochi");
    let mut s = EditorSession::create(&dir, "Audio").map_err(e)?;
    let rep = s.import(&[tone.clone()]);
    let id = rep.imported[0].clone();
    let audio_track = s.project().tracks.iter().find(|t| t.kind == TrackKind::Audio).unwrap().id.clone();
    s.apply(&EditOp::AddClip { media_id: id, track_id: Some(audio_track.clone()), start: Ticks::ZERO }).map_err(e)?;
    let level = |s: &EditorSession| {
        let mixer = mochi_preview::OfflineMixer::new(Arc::new(s.project().clone()), &paths(s));
        let mut buf = vec![0f32; 48_000 * 2];
        mixer.fill(Ticks::from_secs(0.5), &mut buf);
        rms(&buf)
    };
    let base = level(&s);
    let set_vol = |s: &mut EditorSession, v: f64| {
        s.apply(&EditOp::SetTrack { track_id: audio_track.clone(), name: None, locked: None, muted: None, hidden: None, volume_db: Some(v), duck: None, magnetic: None })
    };
    set_vol(&mut s, -12.0).map_err(e)?;
    let lower = level(&s);
    set_vol(&mut s, 99.0).map_err(e)?;
    let stored = s.project().tracks.iter().find(|t| t.id == audio_track).unwrap().volume_db;
    println!("  RMS track 0 dB = {base:.4}; track −12 dB = {lower:.4} → tỉ lệ {:.3} (lý thuyết 0,251)", lower / base);
    println!("  đặt +99 dB → lưu trong project: {stored} dB (giới hạn +12)");
    drop(s);

    // ---------------- kẹp giá trị hiệu ứng ----------------
    println!("\n== Giá trị ngoài giới hạn (kẹp trong project) ==");
    let fake = root.join("Fake.mochi");
    fake_recording::make(&fake, &arrow()?)?;
    let mut s = EditorSession::open(&fake).map_err(e)?;
    let clip = s.project().clips[0].clone();
    let ClipContent::Recording { fx } = &clip.content else { anyhow::bail!("không phải clip quay") };
    let mut bad = fx.clone();
    bad.background.padding = 500.0;
    bad.cursor.size = 1.0;
    bad.camera.size = 90.0;
    bad.camera.border_color = "đỏ".into();
    s.apply(&EditOp::SetRecordingFx { clip_id: clip.id.clone(), fx: bad }).map_err(e)?;
    let ClipContent::Recording { fx: got } = &s.project().clips[0].content else { unreachable!() };
    println!(
        "  gửi padding 500, con trỏ 1%, webcam 90%, màu viền \"đỏ\" → lưu: padding {}, con trỏ {}%, webcam {}%, màu viền {}",
        got.background.padding, got.cursor.size, got.camera.size, got.camera.border_color
    );

    // ---------------- E08 webcam đặt tự do ----------------
    println!("\n== E08 webcam đặt tự do ==");
    let mut fx2 = got.clone();
    fx2.camera.position = Some(NormPoint { x: 0.5, y: 0.5 });
    fx2.camera.hidden_ranges = vec![mochi_project::TimeRange { start: Ticks::from_secs(6.0), end: Ticks::from_secs(7.0) }];
    s.apply(&EditOp::SetRecordingFx { clip_id: clip.id.clone(), fx: fx2 }).map_err(e)?;
    s.save(true).map_err(e)?;
    let ClipContent::Recording { fx: f3 } = &s.project().clips[0].content else { unreachable!() };
    println!("  vị trí lưu: {:?}; đoạn ẩn: {:?}", f3.camera.position, f3.camera.hidden_ranges.len());
    let frames = root.join("frames");
    std::fs::create_dir_all(&frames)?;
    let gpu = mochi_render::GpuContext::new()?;
    let mut renderer = mochi_render::Renderer::new(gpu.clone());
    for (secs, name) in [(1.0, "cam-giua.png"), (6.5, "cam-an.png")] {
        let (tex, _) = renderer.render(s.project(), &paths(&s), Ticks::from_secs(secs), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        image::save_buffer(frames.join(name), &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        println!("  khung {secs}s → {}", frames.join(name).display());
    }

    // ---------------- backup: bản lưu tay + tự lưu thưa ----------------
    println!("\n== Backup ==");
    let before = s.backups();
    println!("  sau Ctrl+S: {} bản, mới nhất manual={}", before.len(), before.first().is_some_and(|b| b.manual));
    s.apply(&EditOp::AddTrack { kind: TrackKind::Video }).map_err(e)?;
    s.mark_major();
    std::thread::sleep(std::time::Duration::from_millis(2100));
    let r = s.autosave_if_due();
    println!(
        "  tự lưu sau thay đổi lớn: {:?}; số backup {} → {} (tự lưu chỉ thêm backup mỗi 10 phút)",
        r.map(|x| x.is_ok()),
        before.len(),
        s.backups().len()
    );
    let tracks_autosaved = s.project().tracks.len();
    drop(s);

    // ---------------- crash → khôi phục có lựa chọn ----------------
    println!("\n== Crash → khôi phục (S02) ==");
    std::fs::write(fake.join(".lock"), "state=open pid=1\n")?; // như app chết giữa chừng
    println!("  is_unclean: {}", mochi_project::package::is_unclean(&fake));
    let s = EditorSession::open(&fake).map_err(e)?;
    println!("  mở lại: recovered={}, số track (bản tự lưu) = {tracks_autosaved} → {}", s.view().recovered, s.project().tracks.len());
    let manual = s.backups().into_iter().find(|b| b.manual).unwrap();
    drop(s);
    let s = EditorSession::open_backup(&fake, &manual.path).map_err(e)?;
    println!(
        "  chọn \"bản bạn lưu\" → số track {} (trước khi thêm track); bản tự lưu được giữ thành backup: {} bản",
        s.project().tracks.len(),
        s.backups().len()
    );
    drop(s);

    // ---------------- project hỏng → chọn backup cũ hơn ----------------
    println!("\n== project.json hỏng ==");
    std::fs::write(fake.join("project.json"), b"{ hong")?;
    let backups = match EditorSession::open(&fake) {
        Err(mochi_editor::EditorError::Project { error: mochi_project::ProjectError::Corrupt { backups, .. } }) => backups,
        other => anyhow::bail!("mong lỗi Corrupt, được {:?}", other.err()),
    };
    println!("  báo hỏng, kèm {} backup để chọn", backups.len());
    std::fs::write(&backups[0].path, b"cung hong")?; // bản mới nhất cũng hỏng
    let r = EditorSession::open_backup(&fake, &backups[0].path);
    println!("  mở bản mới nhất (cũng hỏng) → {}", if r.is_err() { "lỗi, chọn tiếp bản khác" } else { "OK?!" });
    let r = EditorSession::open_backup(&fake, &backups[backups.len() - 1].path);
    println!("  mở bản cũ nhất → {}", if r.is_ok() { "OK" } else { "lỗi" });
    drop(r);

    // ---------------- relink ----------------
    println!("\n== Media thiếu → relink theo tên ==");
    let dir = root.join("Relink.mochi");
    let mut s = EditorSession::create(&dir, "Relink").map_err(e)?;
    let id = s.import(&[clipv.clone()]).imported[0].clone();
    s.save(true).map_err(e)?;
    drop(s);
    let moved = root.join("moved").join("sub");
    std::fs::create_dir_all(&moved)?;
    std::fs::rename(&clipv, moved.join("clip.mp4"))?;
    let mut s = EditorSession::open(&dir).map_err(e)?;
    println!("  thiếu sau khi dời file: {}", s.view().missing.len());
    let r = s.relink_folder(&root.join("moved"));
    println!("  dò thư mục moved/ → nối lại {} media, còn thiếu {}", r.fixed.len(), r.still_missing.len());
    println!("  đường dẫn mới: {:?}", s.media_path(&id));
    drop(s);

    println!("\n== Bản quay bị dời → relink ==");
    let rdir = root.join("Rec.mochi");
    fake_recording::make(&rdir, &arrow()?)?;
    let s = EditorSession::open(&rdir).map_err(e)?;
    drop(s);
    let moved2 = root.join("moved2");
    std::fs::create_dir_all(&moved2)?;
    std::fs::rename(rdir.join("recordings").join("fake0001"), moved2.join("fake0001"))?;
    let mut s = EditorSession::open(&rdir).map_err(e)?;
    println!("  thiếu sau khi dời thư mục bản quay: {}", s.view().missing.len());
    let r = s.relink_folder(&moved2);
    println!("  dò thư mục moved2/ → nối lại {} bản quay, còn thiếu {}", r.fixed.len(), r.still_missing.len());
    let rid = s.project().media[0].id.clone();
    println!("  đường dẫn mới: {:?}", s.media_path(&rid));
    drop(s);

    // ---------------- E01 GIF động ----------------
    println!("\n== E01 GIF động ==");
    let dir = root.join("Gif.mochi");
    let mut s = EditorSession::create(&dir, "Gif").map_err(e)?;
    let id = s.import(&[gif.clone()]).imported[0].clone();
    let item = s.project().media.iter().find(|m| m.id == id).unwrap().clone();
    println!("  import: loại {:?}, độ dài {:.2}s (GIF 2 s)", item.kind, item.duration.secs());
    let vtrack = s.project().tracks.iter().find(|t| t.kind == TrackKind::Video).unwrap().id.clone();
    s.apply(&EditOp::AddClip { media_id: id, track_id: Some(vtrack), start: Ticks::ZERO }).map_err(e)?;
    // Kéo dài clip ảnh ra 4 s để thấy GIF phát lặp.
    let gclip = s.project().clips[0].id.clone();
    s.apply(&EditOp::TrimClip { clip_id: gclip, edge: mochi_timeline::Edge::End, to: Ticks::from_secs(4.0) }).map_err(e)?;
    println!("  clip kéo dài tới {:.2}s", s.project().clips[0].end().secs());
    let mut hashes = vec![];
    for secs in [0.05, 1.05, 2.05] {
        let (tex, _) = renderer.render(s.project(), &paths(&s), Ticks::from_secs(secs), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        rgba.hash(&mut h);
        hashes.push(h.finish());
    }
    println!(
        "  khung 0,05 s ≠ 1,05 s: {}; khung 2,05 s = 0,05 s (lặp lại): {}",
        hashes[0] != hashes[1],
        hashes[2] == hashes[0]
    );
    drop(s);

    // ---------------- proxy bản quay 4K ----------------
    println!("\n== S03 proxy cho bản quay 4K ==");
    let big = root.join("Big.mochi");
    let rec = fake_recording::make_sized(&big, &arrow()?, 3840, 2160)?;
    let s = EditorSession::open(&big).map_err(e)?;
    let item = s.project().media[0].clone();
    println!("  bản quay {}×{} → cần proxy: {}", item.video.unwrap().width, item.video.unwrap().height, mochi_jobs::needs_proxy(&item));
    let dst = mochi_jobs::proxy_path_for(&big, &item);
    let t0 = std::time::Instant::now();
    mochi_jobs::proxy::make_recording(&rec, &dst, &AtomicBool::new(false), |_| {}).map_err(e)?;
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(dst.join("content/segments/segment-0/display.mp4"))
        .output()?;
    println!(
        "  proxy {:?} trong {:.1}s: hình {}, webcam giữ nguyên: {}, dung lượng tính cho cache: {} byte",
        dst.file_name().unwrap(),
        t0.elapsed().as_secs_f64(),
        String::from_utf8_lossy(&probe.stdout).trim(),
        dst.join("content/segments/segment-0/camera.mp4").is_file(),
        std::fs::read_to_string(dst.join(mochi_jobs::proxy::PROXY_BYTES_FILE)).unwrap_or_default()
    );
    let mut with_proxy = paths(&s);
    with_proxy.insert(item.id.clone(), dst.clone());
    let mut r2 = mochi_render::Renderer::new(gpu.clone());
    let (tex, stats) = r2.render(s.project(), &with_proxy, Ticks::from_secs(2.0), true).await;
    let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
    image::save_buffer(frames.join("proxy-4k.png"), &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
    println!("  render qua proxy: {} lớp, thiếu {} → {}", stats.layers, stats.missing_frames, frames.join("proxy-4k.png").display());
    let usage = mochi_jobs::cache::usage(&[big.clone()]);
    println!("  cache.usage: proxies {} byte", usage.proxies_bytes);
    drop(s);

    Ok(())
}
