//! Phase 6 lượt E: phụ đề — đọc SRT, hiện trong hình (burn-in), phụ đề động tô chữ đang nói,
//! xuất kèm .srt theo đoạn xuất (X05). (Phiên âm Whisper cần model — thử riêng khi đã tải model.)

use mochi_editor::EditorSession;
use mochi_project::{Caption, CaptionStyle, CaptionWord, Ticks};
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf};

fn e(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

pub async fn run() -> anyhow::Result<()> {
    let dir = std::env::current_dir()?.join("recordings").join("phase6").join("render").join("V1.mochi");
    anyhow::ensure!(dir.is_dir(), "chạy `mochi-cli phase6-render` trước");
    let mut s = EditorSession::open(&dir).map_err(e)?;
    let srt = "1\r\n00:00:01,000 --> 00:00:03,000\r\nXin chào các bạn\r\n\r\n2\r\n00:00:05,500 --> 00:00:07,000\r\n<i>Dòng thứ hai</i>\r\n\r\n";
    let mut caps = mochi_captions::subtitles::parse(srt);
    println!("== Đọc SRT ==\n  {} dòng: {:?}", caps.len(), caps.iter().map(|c| (c.start.secs(), c.end.secs(), c.text.clone())).collect::<Vec<_>>());
    // Thời gian từng chữ cho dòng 1 (như kết quả Whisper).
    let w = |a: f64, b: f64, t: &str| CaptionWord { start: Ticks::from_secs(a), end: Ticks::from_secs(b), text: t.into() };
    caps[0].words = vec![w(1.0, 1.4, "Xin"), w(1.4, 1.9, "chào"), w(1.9, 2.4, "các"), w(2.4, 3.0, "bạn")];
    s.apply(&EditOp::SetCaptions { captions: caps }).map_err(e)?;
    let style = CaptionStyle { animated: true, size: 6.0, ..Default::default() };
    s.apply(&EditOp::SetCaptionStyle { style: style.clone() }).map_err(e)?;

    let project = s.project().clone();
    let paths: HashMap<String, PathBuf> = project.media.iter().filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p))).collect();
    let gpu = mochi_render::GpuContext::new()?;
    let mut r = mochi_render::Renderer::new(gpu.clone());
    let out = dir.parent().unwrap().join("captions");
    std::fs::create_dir_all(&out)?;
    let mut shot = async |p: &mochi_project::Project, at: f64, name: &str| -> anyhow::Result<Vec<u8>> {
        let (tex, _) = r.render(p, &paths, Ticks::from_secs(at), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        image::save_buffer(out.join(format!("{name}.png")), &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        Ok(rgba)
    };
    let (w_, h_) = (project.canvas.width, project.canvas.height);
    let band = |img: &[u8], pred: &dyn Fn(&[u8]) -> bool| -> usize {
        let mut n = 0;
        for y in (h_ * 78 / 100)..(h_ * 98 / 100) {
            for x in 0..w_ {
                let i = ((y * w_ + x) * 4) as usize;
                if pred(&img[i..i + 3]) {
                    n += 1;
                }
            }
        }
        n
    };
    let yellow = |p: &[u8]| p[0] > 220 && p[1] > 180 && p[2] < 110;
    let mut hidden = project.clone();
    hidden.caption_style.visible = false;
    let plain = shot(&hidden, 1.6, "0-no-captions").await?;
    let a = shot(&project, 1.6, "1-word-2").await?;
    let b = shot(&project, 2.6, "2-word-4").await?;
    let diff = |x: &[u8], y: &[u8]| x.chunks_exact(4).zip(y.chunks_exact(4)).filter(|(p, q)| (0..3).map(|c| (p[c] as i32 - q[c] as i32).abs()).sum::<i32>() > 40).count();
    println!("== Hiện phụ đề (burn-in) ==\n  t=1,6 s: điểm đổi so với tắt phụ đề = {}", diff(&plain, &a));
    println!("== Phụ đề động ==\n  điểm vàng (chữ đang nói) ở dải phụ đề: t=1,6 s ({{chào}}) = {}, t=2,6 s ({{bạn}}) = {}", band(&a, &yellow), band(&b, &yellow));
    let c = shot(&project, 6.0, "3-line-2").await?;
    println!("  dòng 2 (không có thời gian từng chữ) t=6 s: điểm vàng = {} (cả dòng một màu)", band(&c, &yellow));
    let none = shot(&project, 4.0, "4-between").await?;
    println!("  giữa hai dòng t=4 s: điểm đổi so với tắt phụ đề = {}", diff(&none, &shot(&hidden, 4.0, "4-between-plain").await?));

    println!("== X05: xuất 0,5–6 s kèm .srt ==");
    s.save(false).map_err(e)?;
    drop(s);
    let video = out.join("captions-export.mp4");
    let st = std::process::Command::new(std::env::current_exe()?)
        .args(["export"])
        .arg(&dir)
        .args(["--out"])
        .arg(&video)
        .args(["--short", "360", "--range", "0.5,6", "--overwrite"])
        .output()?;
    anyhow::ensure!(st.status.success(), "xuất lỗi: {}", String::from_utf8_lossy(&st.stderr));
    let srt_out = std::fs::read_to_string(video.with_extension("srt"))?;
    println!("  {}:\n{}", video.with_extension("srt").display(), srt_out.trim().lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n"));
    let _ = Caption::clone;
    Ok(())
}
