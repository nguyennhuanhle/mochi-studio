//! Render một vài frame của project bằng đúng `mochi_render::Renderer` (S06) → PNG.

use mochi_editor::EditorSession;
use mochi_project::Ticks;
use std::{collections::HashMap, path::PathBuf, time::Instant};

pub async fn run(project_dir: PathBuf, at: Vec<f64>, out: PathBuf) -> anyhow::Result<()> {
    let session = EditorSession::open(&project_dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let project = session.project().clone();
    let paths: HashMap<String, PathBuf> = project
        .media
        .iter()
        .filter_map(|m| session.media_path(&m.id).map(|p| (m.id.clone(), p)))
        .collect();
    std::fs::create_dir_all(&out)?;
    let t0 = Instant::now();
    let gpu = mochi_render::GpuContext::new()?;
    println!("GPU: {} (khởi tạo {:?})", gpu.adapter_name(), t0.elapsed());
    let mut renderer = mochi_render::Renderer::new(gpu.clone());
    for secs in at {
        let t = Instant::now();
        let (tex, stats) = renderer.render(&project, &paths, Ticks::from_secs(secs), true).await;
        let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
        let file = out.join(format!("frame-{secs:06.2}s.png"));
        image::save_buffer(&file, &rgba, tex.width(), tex.height(), image::ExtendedColorType::Rgba8)?;
        println!(
            "  t={secs:>6.2}s → {} lớp, thiếu {} — {:?} → {}",
            stats.layers,
            stats.missing_frames,
            t.elapsed(),
            file.display()
        );
    }
    for (id, err) in renderer.failures() {
        println!("  lỗi nguồn {id}: {err}");
    }
    Ok(())
}
