//! Vòng 4: xuất từ dòng lệnh qua đúng đường của app (preflight ở tiến trình cha, render + encode ở
//! tiến trình con), và kiểm "preview = export" bằng PSNR.

use mochi_editor::EditorSession;
use mochi_export::{
    ChildExport, ChildMsg, ExportError, ExportFormat, ExportJob, ExportQuality, ExportSettings, VideoCodec,
    preflight,
};
use mochi_project::{Project, Ticks, TimeRange};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Instant,
};

pub fn err(e: &ExportError) -> String {
    serde_json::to_string(e).unwrap_or_default()
}

/// Project + đường dẫn media đang có trên đĩa (mở rồi đóng ngay để nhả .lock).
pub fn load(dir: &Path) -> anyhow::Result<(Project, HashMap<String, PathBuf>)> {
    let s = EditorSession::open(dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let paths = s
        .project()
        .media
        .iter()
        .filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p)))
        .collect();
    Ok((s.project().clone(), paths))
}

pub fn settings(output: &Path) -> ExportSettings {
    ExportSettings {
        format: ExportFormat::Mp4,
        codec: VideoCodec::H264,
        short_side: 1080,
        fps: 30,
        quality: ExportQuality::High,
        range: None,
        output: output.to_path_buf(),
        overwrite: false,
        allow_black_fill: false,
    }
}

pub fn range(a: f64, b: f64) -> Option<TimeRange> {
    Some(TimeRange { start: Ticks::from_secs(a), end: Ticks::from_secs(b) })
}

/// Kết quả một lần xuất qua tiến trình con.
pub struct Run {
    pub result: Result<ChildMsg, ExportError>,
    pub msgs: Vec<String>,
    pub wall: f64,
}

/// Preflight → tiến trình con → chờ xong. `during(child, msg)` được gọi với mỗi tin nhắn tiến độ.
pub fn export(
    project: &Project,
    paths: &HashMap<String, PathBuf>,
    s: &ExportSettings,
    envs: &[(&str, &str)],
    during: impl Fn(&ChildMsg) -> bool + Send + 'static,
) -> Run {
    let started = Instant::now();
    let est = match preflight(project, paths, s) {
        Ok(e) => e,
        Err(e) => return Run { result: Err(e), msgs: vec![], wall: 0.0 },
    };
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let job = ExportJob {
        id: id.clone(),
        project: project.clone(),
        paths: paths.clone(),
        settings: s.clone(),
        output: est.output.clone(),
        log: std::env::current_dir().unwrap().join("recordings/round4/logs").join(format!("export-{id}.log")),
    };
    for (k, v) in envs {
        unsafe { std::env::set_var(k, v) };
    }
    let msgs = Arc::new(Mutex::new(Vec::new()));
    let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (m2, c2) = (msgs.clone(), cancel_flag.clone());
    let exe = std::env::current_exe().unwrap();
    let child = ChildExport::spawn(&exe, &[], &job, move |m| {
        if during(&m) {
            c2.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let line = match &m {
            ChildMsg::Progress { frame, frames, fps, eta_secs } => format!("progress {frame}/{frames} {fps:.1} fps, còn {eta_secs:.1}s"),
            other => serde_json::to_string(other).unwrap(),
        };
        m2.lock().unwrap().push(line);
    });
    for (k, _) in envs {
        unsafe { std::env::remove_var(k) };
    }
    let child = match child {
        Ok(c) => c,
        Err(e) => return Run { result: Err(e), msgs: vec![], wall: 0.0 },
    };
    // Thread đọc chỉ bật cờ; hủy / kill thực hiện ở đây.
    while !child.is_finished() {
        if cancel_flag.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if std::env::var("MOCHI_CLI_KILL_INSTEAD").is_ok() {
                child.kill();
            } else {
                child.cancel();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let result = child.wait();
    let msgs = msgs.lock().unwrap().clone();
    Run { result, msgs, wall: started.elapsed().as_secs_f64() }
}

/// Frame thứ `k` của file video, giải mã bằng ffmpeg (YUV BT.709 giới hạn → RGBA đủ dải).
pub fn decode_frame(file: &Path, k: u32, w: u32, h: u32) -> anyhow::Result<Vec<u8>> {
    let o = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args([
            "-vf",
            &format!("select=eq(n\\,{k}),scale=in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgba"),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()?;
    anyhow::ensure!(o.stdout.len() == (w * h * 4) as usize, "giải mã frame {k}: {} byte", o.stdout.len());
    Ok(o.stdout)
}

/// PSNR (dB) trên 3 kênh màu.
pub fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mut se = 0f64;
    let mut n = 0f64;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for c in 0..3 {
            let d = pa[c] as f64 - pb[c] as f64;
            se += d * d;
            n += 1.0;
        }
    }
    let mse = se / n.max(1.0);
    if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// So frame render trực tiếp (đường của preview) với frame trong file đã xuất; lưu cặp ảnh PNG.
pub async fn psnr_report(
    project: &Project,
    paths: &HashMap<String, PathBuf>,
    s: &ExportSettings,
    file: &Path,
    frames: &[u32],
    out_dir: &Path,
) -> anyhow::Result<()> {
    use mochi_export::settings::{export_range, frame_time, output_size};
    std::fs::create_dir_all(out_dir)?;
    let gpu = mochi_render::GpuContext::new()?;
    let mut r = mochi_render::Renderer::new(gpu.clone());
    let (w, h) = output_size(project, s.short_side);
    let range = export_range(project, s);
    for &k in frames {
        let t = frame_time(&range, s.fps, k);
        let (tex, _) = r.render_sized(project, paths, t, true, Some((w, h))).await;
        let a = mochi_render::read_texture_rgba(&gpu, &tex);
        let b = decode_frame(file, k, w, h)?;
        let p = psnr(&a, &b);
        let preview_png = out_dir.join(format!("frame-{k}-preview.png"));
        image::save_buffer(&preview_png, &a, w, h, image::ColorType::Rgba8)?;
        image::save_buffer(out_dir.join(format!("frame-{k}-export.png")), &b, w, h, image::ColorType::Rgba8)?;
        // Mốc so sánh: chính frame preview đổi sang YUV 4:2:0 BT.709 rồi đổi lại, KHÔNG nén.
        let ideal = yuv420_roundtrip(&preview_png, w, h)?;
        println!(
            "  frame {k:>3} (t={:.2}s): preview↔export {p:.1} dB | preview↔4:2:0 lý tưởng {:.1} dB | export↔4:2:0 lý tưởng {:.1} dB",
            t.secs(),
            psnr(&a, &ideal),
            psnr(&b, &ideal)
        );
    }
    Ok(())
}

fn yuv420_roundtrip(png: &Path, w: u32, h: u32) -> anyhow::Result<Vec<u8>> {
    let o = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(png)
        .args([
            "-vf",
            "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,scale=in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgba",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()?;
    anyhow::ensure!(o.stdout.len() == (w * h * 4) as usize, "roundtrip 4:2:0 lỗi");
    Ok(o.stdout)
}
