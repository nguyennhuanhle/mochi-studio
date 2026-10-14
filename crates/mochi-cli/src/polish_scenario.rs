//! Kịch bản kiểm thử thật cho Vòng 3 (polish): chạy trên bản quay TỔNG HỢP (fake_recording),
//! gọi đúng EditorSession + Renderer của app, in kết quả và xuất PNG để xem bằng mắt.

use mochi_editor::{EditorError, EditorSession};
use mochi_project::{
    BackgroundKind, CameraShape, ClipContent, Corner, Project, Ticks, ZoomTarget,
};
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf};

fn err(e: &EditorError) -> String {
    serde_json::to_string(e).unwrap()
}

fn zooms(p: &Project) -> String {
    p.clips
        .iter()
        .filter_map(|c| match &c.content {
            ClipContent::Recording { fx } => Some(fx.zoom.segments.clone()),
            _ => None,
        })
        .flatten()
        .map(|z| {
            format!(
                "[{:.2}–{:.2}s {}× {}{}]",
                z.start.secs(),
                z.end.secs(),
                z.amount,
                match z.target {
                    ZoomTarget::Follow => "bám con trỏ".to_string(),
                    ZoomTarget::Fixed { x, y } => format!("cố định ({x:.2},{y:.2})"),
                },
                if z.auto { ", auto" } else { ", tay" }
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct Shooter {
    gpu: mochi_render::GpuContext,
    renderer: mochi_render::Renderer,
    out: PathBuf,
}

impl Shooter {
    async fn shot(&mut self, s: &EditorSession, at: f64, name: &str) -> anyhow::Result<()> {
        let p = s.project().clone();
        let paths: HashMap<String, PathBuf> = p
            .media
            .iter()
            .filter_map(|m| s.media_path(&m.id).map(|x| (m.id.clone(), x)))
            .collect();
        let t = std::time::Instant::now();
        let (tex, stats) = self.renderer.render(&p, &paths, Ticks::from_secs(at), true).await;
        let rgba = mochi_render::read_texture_rgba(&self.gpu, &tex);
        let file = self.out.join(format!("{name}.png"));
        image::save_buffer(&file, &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        println!(
            "  ảnh {name}: t={at}s, {}×{}, {} lớp, {:?}",
            tex.width(),
            tex.height(),
            stats.layers,
            t.elapsed()
        );
        Ok(())
    }
}

pub async fn run() -> anyhow::Result<()> {
    let dir = PathBuf::from("recordings/round3/Fake.mochi");
    anyhow::ensure!(dir.join("recordings").is_dir(), "chạy `mochi-cli fake-recording` trước");
    let _ = std::fs::remove_file(dir.join("project.json"));
    let _ = std::fs::remove_dir_all(dir.join("backups"));
    let out = PathBuf::from("recordings/round3/frames");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;

    println!("== R08 + S04: mở bản quay vừa xong → project với auto-polish + zoom tự sinh ==");
    let mut s = EditorSession::open(&dir).map_err(|e| anyhow::anyhow!(err(&e)))?;
    println!("  zoom: {}", zooms(s.project()));
    let rec = s.project().clips[0].id.clone();

    let gpu = mochi_render::GpuContext::new()?;
    let mut shoot = Shooter { renderer: mochi_render::Renderer::new(gpu.clone()), gpu, out };
    shoot.shot(&s, 1.0, "1-polish-mac-dinh").await?;
    shoot.shot(&s, 2.8, "2-zoom-click-1").await?;
    shoot.shot(&s, 6.6, "3-zoom-click-2").await?;

    println!("\n== E06–E08: nền màu, ẩn con trỏ, webcam góc trên-trái bo góc ==");
    let ClipContent::Recording { fx } = &s.project().clips[0].content else { unreachable!() };
    let mut fx = fx.clone();
    fx.background.kind = BackgroundKind::Color;
    fx.background.color = "#F4EEE6".into();
    fx.background.padding = 12.0;
    fx.cursor.hidden = true;
    fx.camera.shape = CameraShape::Rounded;
    fx.camera.corner = Corner::TopLeft;
    s.apply(&EditOp::SetRecordingFx { clip_id: rec.clone(), fx: fx.clone() })?;
    shoot.shot(&s, 2.8, "4-nen-mau-an-con-tro-cam-goc").await?;

    println!("\n== E08: viền webcam (vàng, 4% cạnh ngắn) trên webcam tròn ==");
    let mut fx_border = fx.clone();
    fx_border.camera.shape = CameraShape::Circle;
    fx_border.camera.border = true;
    fx_border.camera.border_color = "#FFC93C".into();
    fx_border.camera.border_width = 4.0;
    s.apply(&EditOp::SetRecordingFx { clip_id: rec.clone(), fx: fx_border })?;
    shoot.shot(&s, 1.0, "4b-vien-webcam").await?;
    s.apply(&EditOp::SetRecordingFx { clip_id: rec.clone(), fx: fx.clone() })?;

    println!("\n== K-E2: hiệu ứng bản quay trên clip không phải bản quay ==");
    let img = PathBuf::from("recordings/bench/logo.png");
    if img.is_file() {
        s.import(&[img]);
        let mid = s.project().media.last().unwrap().id.clone();
        s.apply(&EditOp::AddClip { media_id: mid, track_id: None, start: Ticks::ZERO })?;
        let img_clip = s.project().clips.last().unwrap().id.clone();
        let r = s.apply(&EditOp::SetRecordingFx { clip_id: img_clip.clone(), fx: fx.clone() });
        println!("  SetRecordingFx lên ảnh → {}", r.err().map(|e| err(&e)).unwrap_or("OK?!".into()));
        let r = s.apply(&EditOp::AddZoom { clip_id: img_clip.clone(), start: Ticks::ZERO, end: Ticks::from_secs(1.0), amount: 2.0 });
        println!("  AddZoom lên ảnh → {}", r.err().map(|e| err(&e)).unwrap_or("OK?!".into()));
        s.apply(&EditOp::DeleteClips { clip_ids: vec![img_clip], ripple: false })?;
    }

    println!("\n== E05: sửa tay một vùng zoom + thêm vùng zoom cố định, rồi sinh lại auto ==");
    let first = {
        let ClipContent::Recording { fx } = &s.project().clips[0].content else { unreachable!() };
        fx.zoom.segments.first().map(|z| z.id.clone())
    };
    if let Some(id) = first {
        s.apply(&EditOp::UpdateZoom { clip_id: rec.clone(), zoom_id: id, start: None, end: None, amount: Some(3.0), target: None })?;
    }
    s.apply(&EditOp::AddZoom { clip_id: rec.clone(), start: Ticks::from_secs(4.0), end: Ticks::from_secs(5.2), amount: 9.0 })?;
    println!("  sau khi sửa: {}", zooms(s.project()));
    s.regenerate_zoom(&rec)?;
    println!("  sinh lại auto: {}", zooms(s.project()));

    println!("\n== E10: chữ + vùng pixelate ==");
    s.apply(&EditOp::AddOverlay {
        content: ClipContent::Text {
            text: "Xin chào — Mochi Studio dạy quay màn hình".into(),
            font: "Segoe UI".into(),
            size_px: 54.0,
            color: "#FFFFFF".into(),
            background: Some("#1D1B26CC".into()),
        },
        start: Ticks::ZERO,
        duration: Ticks::from_secs(8.0),
        track_id: None,
    })?;
    s.apply(&EditOp::AddOverlay {
        content: ClipContent::Blur { x: 0.05, y: 0.05, width: 0.3, height: 0.3, pixelate: true, strength: 0.6 },
        start: Ticks::ZERO,
        duration: Ticks::from_secs(8.0),
        track_id: None,
    })?;
    shoot.shot(&s, 1.0, "5-chu-va-pixelate").await?;
    let blur_id = s.project().clips.last().unwrap().id.clone();
    s.apply(&EditOp::SetOverlay {
        clip_id: blur_id,
        content: ClipContent::Blur { x: 0.05, y: 0.05, width: 0.3, height: 0.3, pixelate: false, strength: 0.8 },
    })?;
    shoot.shot(&s, 1.0, "6-lam-mo").await?;

    println!("\n== E09: đổi khung 9:16 ==");
    s.apply(&EditOp::SetCanvas { width: 1080, height: 1920 })?;
    shoot.shot(&s, 2.8, "7-khung-9x16").await?;
    s.apply(&EditOp::SetCanvas { width: 1920, height: 1080 })?;

    println!("\n== E11: tách tiếng ==");
    let vid = PathBuf::from("recordings/round2/media/clip-testsrc.mp4");
    if vid.is_file() {
        s.import(&[vid]);
        let mid = s.project().media.last().unwrap().id.clone();
        s.apply(&EditOp::AddClip { media_id: mid.clone(), track_id: None, start: Ticks::from_secs(9.0) })?;
        let vclip = s.project().clips.iter().find(|c| c.media_id.as_deref() == Some(&mid)).unwrap().id.clone();
        s.apply(&EditOp::DetachAudio { clip_id: vclip.clone() })?;
        let p = s.project();
        let orig = p.clips.iter().find(|c| c.id == vclip).unwrap();
        let audio = p.clips.iter().find(|c| c.media_id.as_deref() == Some(&mid) && c.id != vclip).unwrap();
        let track = p.tracks.iter().find(|t| t.id == audio.track_id).unwrap();
        println!(
            "  clip hình: muted={}; clip tiếng mới trên «{}» {:.2}–{:.2}s, nội dung {:?}",
            orig.muted,
            track.name,
            audio.start.secs(),
            audio.end().secs(),
            serde_json::to_string(&audio.content)?
        );
        let image_clip = PathBuf::from("recordings/bench/logo.png");
        let _ = image_clip;
    }
    s.save(true)?;
    Ok(())
}
