//! Phase 6 lượt C: hiệu ứng của bản quay trên bản quay TỔNG HỢP — spotlight / kính lúp / kiểu con trỏ /
//! nhòe / tiếng click (E06), bố cục / nền / chroma key webcam (E08), overlay phím bấm (R09).
//! Render bằng renderer của app, so ảnh trước / sau và in số đo.

use crate::fake_recording;
use mochi_editor::EditorSession;
use mochi_project::{CameraBackground, CameraLayout, ClipContent, CursorStyle, KeysPosition, RecordingFx, Ticks};
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

fn e(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

fn arrow() -> anyhow::Result<PathBuf> {
    std::fs::read_dir("recordings/round1")?
        .flatten()
        .flat_map(|p| std::fs::read_dir(p.path().join("recordings")).into_iter().flatten().flatten())
        .map(|r| r.path().join("content/cursors/cursor_0.png"))
        .find(|p| p.is_file())
        .ok_or_else(|| anyhow::anyhow!("cần ảnh con trỏ từ một bản quay Vòng 1"))
}

/// Số điểm ảnh khác nhau (tổng chênh RGB > 24) trong vùng [x0, x1) × [y0, y1).
fn changed(a: &[u8], b: &[u8], w: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> usize {
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * w + x) * 4) as usize;
            let d: i32 = (0..3).map(|c| (a[i + c] as i32 - b[i + c] as i32).abs()).sum();
            if d > 24 {
                n += 1;
            }
        }
    }
    n
}

fn lum(a: &[u8], w: u32, x: u32, y: u32) -> u32 {
    let i = ((y * w + x) * 4) as usize;
    (a[i] as u32 + a[i + 1] as u32 + a[i + 2] as u32) / 3
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase6").join("recording");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let dir = root.join("Rec.mochi");
    let rec = fake_recording::make(&dir, &arrow()?)?;
    // R09: dữ liệu phím (Ctrl+C lúc 2 s) → thêm vào meta của bản quay tổng hợp.
    let seg = rec.join("content/segments/segment-0");
    let key = |k: &str, code: &str, t: f64, down: bool| serde_json::json!({"key": k, "keyCode": code, "timeMs": t * 1000.0, "down": down});
    std::fs::write(
        seg.join("keyboard.json"),
        serde_json::to_vec(&serde_json::json!({"presses": [
            key("LControl", "LControl", 2.0, true), key("c", "KeyC", 2.05, true),
            key("c", "KeyC", 2.25, false), key("LControl", "LControl", 2.3, false)
        ]}))?,
    )?;
    let meta_path = rec.join("recording-meta.json");
    let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path)?)?;
    meta["segments"][0]["keyboard"] = "content/segments/segment-0/keyboard.json".into();
    std::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)?;

    let mut s = EditorSession::open(&dir).map_err(e)?;
    let clip = s.project().clips[0].clone();
    let ClipContent::Recording { fx } = &clip.content else { anyhow::bail!("không phải clip quay") };
    let paths: HashMap<String, PathBuf> = s.project().media.iter().filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p))).collect();
    let gpu = mochi_render::GpuContext::new()?;
    let mut renderer = mochi_render::Renderer::new(gpu.clone());
    let frames = root.join("frames");
    std::fs::create_dir_all(&frames)?;
    let base = {
        let mut f: RecordingFx = fx.clone();
        f.cursor.hide_when_idle = false;
        f.zoom.segments.clear();
        f
    };
    let (w, h) = (s.project().canvas.width, s.project().canvas.height);
    let mut render = async |s: &mut EditorSession, f: &RecordingFx, secs: f64, name: &str| -> anyhow::Result<Vec<u8>> {
        s.apply(&EditOp::SetRecordingFx { clip_id: clip.id.clone(), fx: f.clone() }).map_err(e)?;
        let (tex, _) = renderer.render(s.project(), &paths, Ticks::from_secs(secs), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        image::save_buffer(frames.join(format!("{name}.png")), &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        Ok(rgba)
    };
    println!("  khung {w}×{h}");
    let plain = render(&mut s, &base, 3.0, "0-plain").await?;
    // Con trỏ đứng ở (0.72, 0.62) của bản quay lúc 1,8–4,5 s; tìm vị trí trên khung qua highlight không cần — đo vùng.
    let full = (0, 0, w, h);

    println!("== E06 spotlight ==");
    let mut f = base.clone();
    f.cursor.spotlight.enabled = true;
    f.cursor.spotlight.dim = 0.7;
    let spot = render(&mut s, &f, 3.0, "1-spotlight").await?;
    println!("  góc trên trái (60,60): {} → {} (tối đi); điểm đổi trên cả khung: {}", lum(&plain, w, 60, 60), lum(&spot, w, 60, 60), changed(&plain, &spot, w, full));

    println!("== E06 kính lúp ==");
    let mut f = base.clone();
    f.cursor.magnifier.enabled = true;
    f.cursor.magnifier.zoom = 3.0;
    let mag = render(&mut s, &f, 3.0, "2-magnifier").await?;
    let n_mag = changed(&plain, &mag, w, full);
    println!("  điểm đổi: {n_mag} (≈ diện tích kính lúp π·r², r = 16% cạnh ngắn = {:.0} px → {:.0})", 0.16 * h as f64, std::f64::consts::PI * (0.16 * h as f64).powi(2));

    println!("== E06 kiểu con trỏ ==");
    for (style, name) in [(CursorStyle::MacOs, "3-cursor-macos"), (CursorStyle::Windows, "3-cursor-windows")] {
        let mut f = base.clone();
        f.cursor.style = style;
        let img = render(&mut s, &f, 3.0, name).await?;
        println!("  {style:?}: điểm đổi so với con trỏ đã ghi = {}", changed(&plain, &img, w, full));
    }

    println!("== E06 nhòe con trỏ khi di chuyển (t = 0,9 s, đang di chuyển) ==");
    let moving = render(&mut s, &base, 0.9, "4-moving").await?;
    let mut f = base.clone();
    f.cursor.motion_blur = 1.0;
    let blurred = render(&mut s, &f, 0.9, "4-moving-blur").await?;
    println!("  điểm đổi khi bật nhòe = {}", changed(&moving, &blurred, w, full));

    println!("== R09 overlay phím bấm (Ctrl+C lúc 2,0–2,3 s) ==");
    let at_keys = render(&mut s, &base, 2.15, "5-no-keys").await?;
    let mut f = base.clone();
    f.keys.enabled = true;
    f.keys.position = KeysPosition::BottomCenter;
    let keys = render(&mut s, &f, 2.15, "5-keys").await?;
    let bottom = (w / 4, h * 3 / 4, w * 3 / 4, h);
    println!("  dưới giữa: điểm đổi = {} (cả khung {})", changed(&at_keys, &keys, w, bottom), changed(&at_keys, &keys, w, full));
    let later = render(&mut s, &f, 4.0, "5-keys-later").await?;
    let later_plain = render(&mut s, &base, 4.0, "5-no-keys-later").await?;
    println!("  t = 4 s (đã nhả phím lâu): điểm đổi = {}", changed(&later_plain, &later, w, full));

    println!("== E08 bố cục webcam ==");
    for (layout, name) in [(CameraLayout::CameraOnly, "6-camera-only"), (CameraLayout::Split, "6-split"), (CameraLayout::Floating, "6-floating")] {
        let mut f = base.clone();
        f.camera.layout = layout;
        let img = render(&mut s, &f, 3.0, name).await?;
        println!("  {layout:?}: điểm đổi so với PiP = {} / {}", changed(&plain, &img, w, full), w * h);
    }

    println!("== E08 nền webcam (ONNX Runtime CPU) ==");
    for (bg, name) in [(CameraBackground::Blur, "7-cam-blur"), (CameraBackground::Remove, "7-cam-remove")] {
        // Webcam ở góc (PiP): xóa nền = cắt người ra khỏi nền, đặt lên màn hình (Cap không cắt ở chế độ chỉ webcam).
        let mut f = base.clone();
        f.camera.background = bg;
        let img = render(&mut s, &f, 3.0, name).await?;
        println!("  {bg:?}: điểm đổi so với giữ nguyên = {}", changed(&plain, &img, w, full));
    }

    println!("== E08 chroma key ==");
    let mut f = base.clone();
    f.camera.layout = CameraLayout::CameraOnly;
    let only = render(&mut s, &f, 3.0, "8-cam-only").await?;
    // Màu khóa = màu tại tâm khung webcam (thanh màu SMPTE) → vùng đó trong suốt.
    let i = (((h / 2) * w + w / 2) * 4) as usize;
    let key = format!("#{:02X}{:02X}{:02X}", only[i], only[i + 1], only[i + 2]);
    f.camera.chroma.enabled = true;
    f.camera.chroma.color = key.clone();
    f.camera.chroma.tolerance = 25.0;
    let keyed = render(&mut s, &f, 3.0, "8-chroma").await?;
    println!("  màu khóa {key}: tâm {} → {}; điểm đổi = {}", lum(&only, w, w / 2, h / 2), lum(&keyed, w, w / 2, h / 2), changed(&only, &keyed, w, full));

    println!("== E06 tiếng click ==");
    let mut f = base.clone();
    f.cursor.click_sound.enabled = true;
    f.cursor.click_sound.volume = 80.0;
    s.apply(&EditOp::SetRecordingFx { clip_id: clip.id.clone(), fx: f }).map_err(e)?;
    let clicks = mochi_project::autozoom::click_times(&rec);
    let mixer = mochi_preview::OfflineMixer::new(Arc::new(s.project().clone()), &paths);
    let peak = |from: f64, to: f64| {
        let n = ((to - from) * mochi_preview::OfflineMixer::RATE as f64) as usize;
        let mut buf = vec![0f32; n * 2];
        mixer.fill(Ticks::from_secs(from), &mut buf);
        buf.iter().fold(0f32, |m, v| m.max(v.abs()))
    };
    println!("  click lúc {:?} s; có tiếng: {}", clicks.iter().map(|c| format!("{c:.2}")).collect::<Vec<_>>(), mixer.has_audio());
    if let Some(&c) = clicks.first() {
        println!("  biên độ lớn nhất: 50 ms ngay sau click = {:.3}; 50 ms trước click = {:.3}", peak(c, c + 0.05), peak((c - 0.1).max(0.0), c - 0.05));
    }
    println!("  ảnh: {}", frames.display());
    Ok(())
}
