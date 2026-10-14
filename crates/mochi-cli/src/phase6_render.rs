//! Phase 6 lượt B: kiểm thử thật compositor v1 trên media tổng hợp — keyframe + xoay (E12),
//! chuyển cảnh (E12), màu / viền / bóng / khung thiết bị (E15), hình khối / mũi tên / callout /
//! spotlight (E10), gõ chữ. Render bằng ĐÚNG `Renderer` của preview / xuất, lưu PNG và in số đo.

use mochi_editor::EditorSession;
use mochi_project::{
    ClipContent, DeviceFrame, Easing, Keyframe, Look, ShapeKind, Ticks, Transition, TransitionKind,
};
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf, process::Command};

fn ffmpeg(args: &[&str]) {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "ffmpeg {args:?} thất bại");
}

fn secs(s: f64) -> Ticks {
    Ticks::from_secs(s)
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase6").join("render");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let a = root.join("a-testsrc.mp4");
    let b = root.join("b-bars.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p", a.to_str().unwrap()]);
    ffmpeg(&["-f", "lavfi", "-i", "smptehdbars=size=1280x720:rate=30:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p", b.to_str().unwrap()]);

    let dir = root.join("V1.mochi");
    let mut s = EditorSession::create(&dir, "V1").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let r = s.import(&[a.clone(), b.clone()]);
    anyhow::ensure!(r.imported.len() == 2, "import lỗi: {:?}", r.rejected);
    let ap = |s: &mut EditorSession, op: EditOp| s.apply(&op).map_err(|e| anyhow::anyhow!(format!("{op:?}: {e}")));
    ap(&mut s, EditOp::SetCanvas { width: 1280, height: 720 })?;
    let media: Vec<String> = s.project().media.iter().map(|m| m.id.clone()).collect();
    let v1 = s.project().tracks[0].id.clone();
    for (i, (m, start)) in [(0usize, 0.0), (1, 4.0), (0, 8.0)].into_iter().enumerate() {
        ap(&mut s, EditOp::AddClip { media_id: media[m].clone(), track_id: Some(v1.clone()), start: secs(start) })?;
        let _ = i;
    }
    let clip_at = |s: &EditorSession, at: f64| s.project().clips.iter().find(|c| c.start == secs(at)).unwrap().id.clone();
    let (c1, c2, c3) = (clip_at(&s, 0.0), clip_at(&s, 4.0), clip_at(&s, 8.0));
    // c2 trượt vào từ phải (1 s), c3 hòa vào (1 s) và wipe ra ở cuối (1 s).
    ap(&mut s, EditOp::SetTransition { clip_id: c2.clone(), out: false, transition: Some(Transition { kind: TransitionKind::SlideLeft, duration: secs(1.0) }) })?;
    ap(&mut s, EditOp::SetTransition { clip_id: c3.clone(), out: false, transition: Some(Transition { kind: TransitionKind::Fade, duration: secs(1.0) }) })?;
    ap(&mut s, EditOp::SetTransition { clip_id: c3.clone(), out: true, transition: Some(Transition { kind: TransitionKind::WipeRight, duration: secs(1.0) }) })?;
    // E15: laptop + bóng cho c1; điện thoại cho c2; trình duyệt + trắng đen + sáng hơn cho c3.
    ap(&mut s, EditOp::SetLook { clip_id: c1.clone(), look: Look { device: DeviceFrame::Laptop, shadow: 60.0, ..Default::default() } })?;
    ap(&mut s, EditOp::SetLook { clip_id: c2.clone(), look: Look { device: DeviceFrame::Phone, shadow: 40.0, ..Default::default() } })?;
    ap(&mut s, EditOp::SetLook { clip_id: c3.clone(), look: Look { device: DeviceFrame::Browser, saturation: -100.0, brightness: 20.0, border_width: 6.0, border_color: "#E2557B".into(), corner_radius: 4.0, ..Default::default() } })?;

    // Overlay: chữ chạy keyframe (trái → phải, xoay 0 → 90°), mũi tên, khung chữ nhật, callout, spotlight.
    let overlay = |content: ClipContent, start: f64, dur: f64| EditOp::AddOverlay { content, start: secs(start), duration: secs(dur), track_id: None };
    ap(&mut s, overlay(ClipContent::Text { text: "Keyframe ✓".into(), font: "Segoe UI".into(), size_px: 56.0, color: "#FFFFFF".into(), background: Some("#1D1B26CC".into()) }, 0.0, 3.0))?;
    let text_id = s.project().clips.iter().find(|c| matches!(c.content, ClipContent::Text { .. })).unwrap().id.clone();
    let kf = |at: f64, x: f64, rot: f64| Keyframe { at: secs(at), x, y: 0.2, scale: 1.0, rotation: rot, opacity: 1.0, easing: Easing::Linear };
    ap(&mut s, EditOp::SetKeyframes { clip_id: text_id.clone(), keyframes: vec![kf(0.0, 0.2, 0.0), kf(2.0, 0.8, 90.0)] })?;
    ap(&mut s, overlay(ClipContent::Shape { shape: ShapeKind::Arrow, x0: 0.1, y0: 0.85, x1: 0.35, y1: 0.6, stroke: "#E2557B".into(), stroke_width: 8.0, fill: None }, 4.0, 4.0))?;
    ap(&mut s, overlay(ClipContent::Shape { shape: ShapeKind::Rectangle, x0: 0.6, y0: 0.55, x1: 0.9, y1: 0.85, stroke: "#FFD54A".into(), stroke_width: 6.0, fill: Some("#FFD54A33".into()) }, 4.0, 4.0))?;
    ap(&mut s, overlay(ClipContent::Callout { text: "Bấm vào đây".into(), font: "Segoe UI".into(), size_px: 36.0, color: "#1D1B26".into(), background: "#FFD54A".into(), tail_x: 0.3, tail_y: 0.75 }, 5.0, 3.0))?;
    ap(&mut s, overlay(ClipContent::Spotlight { x: 0.35, y: 0.3, width: 0.3, height: 0.4, ellipse: true, dim: 0.7 }, 9.0, 1.5))?;
    ap(&mut s, overlay(ClipContent::Text { text: "Gõ từng chữ…".into(), font: "Segoe UI".into(), size_px: 48.0, color: "#FFD54A".into(), background: None }, 8.0, 3.0))?;
    let typed = s.project().clips.iter().find(|c| matches!(&c.content, ClipContent::Text { text, .. } if text.starts_with("Gõ"))).unwrap().id.clone();
    ap(&mut s, EditOp::SetTransition { clip_id: typed.clone(), out: false, transition: Some(Transition { kind: TransitionKind::Typewriter, duration: secs(1.5) }) })?;
    // Luật: gõ chữ không dùng cho video.
    match s.apply(&EditOp::SetTransition { clip_id: c1.clone(), out: false, transition: Some(Transition { kind: TransitionKind::Typewriter, duration: secs(1.0) }) }) {
        Ok(_) => println!("  ✗ gõ chữ trên video lẽ ra phải bị từ chối"),
        Err(e) => println!("  gõ chữ trên clip video → từ chối: {e}"),
    }
    // Kẹp giá trị: look ngoài giới hạn.
    let _ = s.apply(&EditOp::SetLook { clip_id: c1.clone(), look: Look { brightness: 999.0, border_width: -5.0, border_color: "đỏ".into(), device: DeviceFrame::Laptop, shadow: 60.0, ..Default::default() } });
    let l = s.project().clips.iter().find(|c| c.id == c1).unwrap().look.clone();
    println!("  look ngoài giới hạn → sáng {}, viền {}, màu viền {}", l.brightness, l.border_width, l.border_color);
    ap(&mut s, EditOp::SetLook { clip_id: c1.clone(), look: Look { device: DeviceFrame::Laptop, shadow: 60.0, ..Default::default() } })?;
    s.save(false).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let project = s.project().clone();
    let paths: HashMap<String, PathBuf> = project.media.iter().filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p))).collect();
    let gpu = mochi_render::GpuContext::new()?;
    let mut renderer = mochi_render::Renderer::new(gpu.clone());
    let frames = root.join("frames");
    std::fs::create_dir_all(&frames)?;
    let mut shots: HashMap<String, Vec<u8>> = HashMap::new();
    for at in [1.0, 2.5, 4.25, 4.5, 4.75, 5.5, 8.5, 8.75, 9.5, 11.5] {
        let (tex, st) = renderer.render(&project, &paths, secs(at), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        let file = frames.join(format!("t{at:05.2}.png"));
        image::save_buffer(&file, &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        println!("  t={at:>5.2}s: {} lớp, thiếu {} → {}", st.layers, st.missing_frames, file.file_name().unwrap().to_string_lossy());
        shots.insert(format!("{at:.2}"), rgba);
    }
    let px = |k: &str, x: u32, y: u32| {
        let i = ((y * 1280 + x) * 4) as usize;
        let v = &shots[k];
        [v[i], v[i + 1], v[i + 2]]
    };
    // Keyframe (tuyến tính 0 s: x 0.2, 0° → 2 s: x 0.8, 90°): giá trị nội suy từ model.
    let text = project.clips.iter().find(|c| c.id == text_id).unwrap();
    for at in [0.0, 1.0, 1.5, 2.5] {
        let tf = text.transform_at(secs(at));
        println!("  E12 keyframe t={at:.1}s: x = {:.3}, xoay = {:.1}°", tf.x, tf.rotation);
    }
    // Slide: tại t=4.5 s clip sau đã vào nửa khung → cột 1000 thuộc clip sau, cột 100 thuộc clip trước.
    println!("  E12 slide t=4.25/4.5/4.75: điểm (1200, 360) = {:?} / {:?} / {:?}", px("4.25", 1200, 360), px("4.50", 1200, 360), px("4.75", 1200, 360));
    // Fade: clip 3 hòa vào 25 % ở t = 8.25 → t=8.5 là 50 %.
    let gray = |k: &str| {
        let p = px(k, 640, 400);
        (p[0] as i32 - p[1] as i32).abs().max((p[1] as i32 - p[2] as i32).abs())
    };
    println!("  E15 trắng đen (bão hòa −100) t=9.5 s: lệch kênh lớn nhất ở (640,400) = {} (≈0 là xám)", gray("9.50"));
    let lum = |k: &str, x: u32, y: u32| { let p = px(k, x, y); (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 };
    // Spotlight: so cùng khung khi bỏ lớp spotlight → trong vùng sáng giữ nguyên, ngoài tối đi ~70 %.
    let mut no_spot = project.clone();
    no_spot.clips.retain(|c| !matches!(c.content, ClipContent::Spotlight { .. }));
    let (tex, _) = renderer.render(&no_spot, &paths, secs(9.5), true).await;
    let plain = mochi_render::read_texture_rgba(&gpu, &tex);
    let lum_plain = |x: u32, y: u32| { let i = ((y * 1280 + x) * 4) as usize; (plain[i] as u32 + plain[i + 1] as u32 + plain[i + 2] as u32) / 3 };
    println!(
        "  E10 spotlight t=9.5 s: tâm vùng sáng (640,360) {} → {} (giữ nguyên), ngoài vùng (300,250) {} → {} (tối đi)",
        lum_plain(640, 360), lum("9.50", 640, 360), lum_plain(300, 250), lum("9.50", 300, 250)
    );
    println!("  E12 wipe ra t=11.5 s: trái (200,360) sáng {}, phải (1100,360) sáng {} (nửa phải đã bị wipe)", lum("11.50", 200, 360), lum("11.50", 1100, 360));
    println!("  ảnh: {}", frames.display());

    // X02 (v1): WebM nền trong suốt — project chỉ có chữ + mũi tên (không có video nền).
    let adir = root.join("Alpha.mochi");
    let mut a = EditorSession::create(&adir, "Alpha").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    ap(&mut a, EditOp::SetCanvas { width: 640, height: 360 })?;
    ap(&mut a, EditOp::AddOverlay { content: ClipContent::Text { text: "Trong suốt".into(), font: "Segoe UI".into(), size_px: 48.0, color: "#FFFFFF".into(), background: Some("#E2557BFF".into()) }, start: secs(0.0), duration: secs(1.0), track_id: None })?;
    ap(&mut a, EditOp::AddOverlay { content: ClipContent::Shape { shape: ShapeKind::Arrow, x0: 0.1, y0: 0.2, x1: 0.4, y1: 0.4, stroke: "#FFD54A".into(), stroke_width: 8.0, fill: None }, start: secs(0.0), duration: secs(1.0), track_id: None })?;
    a.save(false).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    drop(a);
    let webm = root.join("alpha.webm");
    let st = Command::new(std::env::current_exe()?).arg("export").arg(&adir).arg("--out").arg(&webm).args(["--format", "webm", "--short", "360", "--overwrite"]).output()?;
    anyhow::ensure!(st.status.success(), "xuất webm lỗi: {}", String::from_utf8_lossy(&st.stdout));
    let dec = Command::new("ffmpeg")
        .args(["-v", "error", "-c:v", "libvpx-vp9", "-i"])
        .arg(&webm)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()?;
    let px = |x: usize, y: usize| dec.stdout.get((y * 640 + x) * 4 + 3).copied();
    println!("  X02 WebM: {} byte; alpha góc (5,5) = {:?}, giữa chữ (320,306) = {:?}", std::fs::metadata(&webm)?.len(), px(5, 5), px(320, 306));
    Ok(())
}
