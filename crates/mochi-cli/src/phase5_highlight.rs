//! Phase 5 (R13 / E06): highlight con trỏ trên bản quay TỔNG HỢP. Render thật bằng renderer của app,
//! so ảnh có / không có highlight, in vùng điểm ảnh bị đổi (tâm, cỡ) — không cần mở ảnh để kết luận.

use crate::fake_recording;
use mochi_editor::EditorSession;
use mochi_project::{ClipContent, CursorHighlight, RecordingFx, Ticks};
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf};

fn e(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

/// Vùng điểm ảnh khác nhau giữa hai ảnh RGBA: (số điểm, tâm x, tâm y, rộng, cao).
fn diff(a: &[u8], b: &[u8], w: u32) -> (usize, f64, f64, u32, u32) {
    let (mut n, mut sx, mut sy) = (0usize, 0f64, 0f64);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, (pa, pb)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        let d: i32 = (0..3).map(|c| (pa[c] as i32 - pb[c] as i32).abs()).sum();
        if d > 24 {
            let (x, y) = (i as u32 % w, i as u32 / w);
            n += 1;
            sx += x as f64;
            sy += y as f64;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if n == 0 {
        return (0, 0.0, 0.0, 0, 0);
    }
    (n, sx / n as f64, sy / n as f64, x1 - x0 + 1, y1 - y0 + 1)
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
    let root = std::env::current_dir()?.join("recordings").join("phase5");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let dir = root.join("Hl.mochi");
    let rec = fake_recording::make(&dir, &arrow()?)?;

    // R13: lựa chọn lúc quay (ghi vào sidecar khi dừng) → clip mới mang sẵn highlight.
    let mut side = mochi_project::sidecar::RecordingSidecar::read(&rec);
    side.cursor_highlight = Some(CursorHighlight { enabled: true, color: "#FFD54A".into(), size: 200.0, opacity: 60.0 });
    side.write(&rec)?;
    let mut s = EditorSession::open(&dir).map_err(e)?;
    let clip = s.project().clips[0].clone();
    let ClipContent::Recording { fx } = &clip.content else { anyhow::bail!("không phải clip quay") };
    println!("== R13: bật highlight lúc quay → clip mới ==");
    println!("  fx.cursor.highlight = {:?}", fx.cursor.highlight);

    let gpu = mochi_render::GpuContext::new()?;
    let mut renderer = mochi_render::Renderer::new(gpu.clone());
    let out = root.join("frames");
    std::fs::create_dir_all(&out)?;
    let paths: HashMap<String, PathBuf> =
        s.project().media.iter().filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p))).collect();

    // Bỏ webcam để không che con trỏ; con trỏ không tự ẩn (trừ bài "đứng yên").
    let base = {
        let mut f: RecordingFx = fx.clone();
        f.camera.visible = false;
        f.cursor.hide_when_idle = false;
        f
    };
    let mut render = async |s: &mut EditorSession, f: &RecordingFx, secs: f64, name: &str| -> anyhow::Result<(Vec<u8>, u32)> {
        s.apply(&EditOp::SetRecordingFx { clip_id: clip.id.clone(), fx: f.clone() }).map_err(e)?;
        let (tex, _) = renderer.render(s.project(), &paths, Ticks::from_secs(secs), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        image::save_buffer(out.join(name), &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        Ok((rgba, tex.width()))
    };
    let with = |on: bool, f: &RecordingFx| {
        let mut f = f.clone();
        f.cursor.highlight.enabled = on;
        f
    };

    println!("\n== Highlight bật / tắt (khác biệt điểm ảnh quanh con trỏ) ==");
    for (secs, label) in [(1.0, "con trỏ đang di chuyển"), (2.3, "đang zoom (click ở 2,0 s)"), (5.0, "con trỏ đứng yên, không zoom")] {
        let (off, w) = render(&mut s, &with(false, &base), secs, &format!("t{secs}-off.png")).await?;
        let (on, _) = render(&mut s, &with(true, &base), secs, &format!("t{secs}-on.png")).await?;
        let (n, cx, cy, bw, bh) = diff(&off, &on, w);
        println!("  t={secs:>4}s ({label}): {n} điểm đổi, tâm ({cx:.0}, {cy:.0}), vùng {bw}×{bh}");
    }

    println!("\n== Highlight ẩn theo con trỏ ==");
    let mut hidden = base.clone();
    hidden.cursor.hidden = true;
    let (off, w) = render(&mut s, &with(false, &hidden), 1.0, "hidden-off.png").await?;
    let (on, _) = render(&mut s, &with(true, &hidden), 1.0, "hidden-on.png").await?;
    println!("  tắt con trỏ: {} điểm đổi (phải = 0)", diff(&off, &on, w).0);
    // Bản quay riêng: con trỏ chỉ di chuyển trong 1 s đầu, không click, rồi đứng yên tới hết.
    let idle_dir = root.join("Idle.mochi");
    let idle_rec = fake_recording::make(&idle_dir, &arrow()?)?;
    let cursor_json = idle_rec.join("content/segments/segment-0/cursor.json");
    let mut cj: serde_json::Value = serde_json::from_slice(&std::fs::read(&cursor_json)?)?;
    if let Some(moves) = cj["moves"].as_array_mut() {
        moves.retain(|m| m["time_ms"].as_f64().unwrap_or(0.0) < 1000.0);
    }
    cj["clicks"] = serde_json::json!([]);
    std::fs::write(&cursor_json, serde_json::to_vec_pretty(&cj)?)?;
    let mut si = EditorSession::open(&idle_dir).map_err(e)?;
    let iclip = si.project().clips[0].clone();
    let ipaths: HashMap<String, PathBuf> =
        si.project().media.iter().filter_map(|m| si.media_path(&m.id).map(|p| (m.id.clone(), p))).collect();
    let mut idle = base.clone();
    idle.cursor.hide_when_idle = true;
    idle.zoom.segments.clear();
    let mut r2 = mochi_render::Renderer::new(gpu.clone());
    let mut frame = async |si: &mut EditorSession, f: RecordingFx, secs: f64| -> anyhow::Result<(Vec<u8>, u32)> {
        si.apply(&EditOp::SetRecordingFx { clip_id: iclip.id.clone(), fx: f }).map_err(e)?;
        let (tex, _) = r2.render(si.project(), &ipaths, Ticks::from_secs(secs), true).await;
        Ok((mochi_render::read_texture_rgba(&gpu, &tex), tex.width()))
    };
    for secs in [1.2, 6.0] {
        let (off, w) = frame(&mut si, with(false, &idle), secs).await?;
        let (on, _) = frame(&mut si, with(true, &idle), secs).await?;
        let mut no_idle = idle.clone();
        no_idle.cursor.hide_when_idle = false;
        let (shown, _) = frame(&mut si, with(false, &no_idle), secs).await?;
        println!(
            "  tự ẩn khi đứng yên, t={secs}s: con trỏ {} · highlight đổi {} điểm",
            if diff(&off, &shown, w).0 > 0 { "đã ẩn" } else { "còn hiện" },
            diff(&off, &on, w).0
        );
    }

    println!("\n== Cỡ / độ đậm ==");
    for (size, opacity) in [(100.0, 45.0), (300.0, 45.0), (200.0, 100.0)] {
        let mut f = with(true, &base);
        f.cursor.highlight.size = size;
        f.cursor.highlight.opacity = opacity;
        let (off, w) = render(&mut s, &with(false, &base), 5.0, "sz-off.png").await?;
        let (on, _) = render(&mut s, &f, 5.0, &format!("sz{size}-op{opacity}.png")).await?;
        let (n, _, _, bw, bh) = diff(&off, &on, w);
        let peak = off
            .chunks_exact(4)
            .zip(on.chunks_exact(4))
            .map(|(a, b)| (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).sum::<i32>())
            .max()
            .unwrap_or(0);
        println!("  cỡ {size}% · độ đậm {opacity}%: vùng {bw}×{bh} ({n} điểm), đổi màu lớn nhất {peak}");
    }

    println!("\n== Giá trị ngoài giới hạn ==");
    let mut bad = with(true, &base);
    bad.cursor.highlight.size = 9999.0;
    bad.cursor.highlight.opacity = -5.0;
    bad.cursor.highlight.color = "vàng".into();
    s.apply(&EditOp::SetRecordingFx { clip_id: clip.id.clone(), fx: bad }).map_err(e)?;
    let ClipContent::Recording { fx: got } = &s.project().clips[0].content else { unreachable!() };
    println!("  gửi cỡ 9999, độ đậm −5, màu \"vàng\" → lưu {:?}", got.cursor.highlight);
    println!("\n  ảnh: {}", out.display());
    Ok(())
}
