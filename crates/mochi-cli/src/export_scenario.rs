//! Kịch bản kiểm thử thật Vòng 4 (Người xuất bản) trên media TỔNG HỢP. In ra kết quả cụ thể:
//! encoder máy hỗ trợ, từng luật K-X, file xuất thật (ffprobe), lỗi giữa chừng, hàng đợi, PSNR, A/V sync.

use crate::export_cmd::{self, Run, err, load, range, settings};
use mochi_editor::EditorSession;
use mochi_export::{
    ChildMsg, ExportError, ExportFormat, ExportQueue, ExportQuality, VideoCodec, encoder_capabilities, preflight,
};
use mochi_project::{ClipContent, Ticks};
use mochi_timeline::EditOp;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

pub fn root() -> PathBuf {
    std::env::current_dir().unwrap().join("recordings").join("round4")
}

fn ffmpeg(args: &[&str]) {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "ffmpeg {args:?} lỗi");
}

pub fn ffprobe(file: &Path) -> String {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_name,width,height,r_frame_rate,sample_rate:format=duration,size", "-of", "compact=p=0:nk=0"])
        .arg(file)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace('\n', " | "))
        .unwrap_or_default();
    out
}

fn show(label: &str, r: &Run) {
    match &r.result {
        Ok(ChildMsg::Done { path, bytes, secs, encoder }) => println!(
            "  {label}: XONG bằng {encoder} trong {secs:.1}s (cả khởi động tiến trình: {:.1}s) → {} ({:.2} MB)",
            r.wall,
            path.file_name().unwrap().to_string_lossy(),
            *bytes as f64 / 1e6
        ),
        Ok(other) => println!("  {label}: {other:?}"),
        Err(e) => println!("  {label}: LỖI {}", err(e)),
    }
    for m in r.msgs.iter().filter(|m| !m.starts_with("progress")) {
        println!("      · {m}");
    }
    if let Some(last) = r.msgs.iter().rev().find(|m| m.starts_with("progress")) {
        println!("      · tiến độ cuối: {last}");
    }
}

/// Project thử: bản quay tổng hợp (polish mặc định + zoom tự sinh) + chữ + nhạc nền.
fn build_project(dir: &Path) -> anyhow::Result<()> {
    let _ = std::fs::remove_dir_all(dir);
    copy_dir(Path::new("recordings/round3/Fake.mochi"), dir)?;
    for f in ["project.json", ".lock"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let _ = std::fs::remove_dir_all(dir.join("backups"));
    let mut s = EditorSession::open(dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let music = PathBuf::from("recordings/round2/chuyen-di/nhac-nen.mp3");
    s.import(&[std::path::absolute(&music)?]);
    let mid = s.project().media.last().unwrap().id.clone();
    s.apply(&EditOp::AddClip { media_id: mid, track_id: None, start: Ticks::ZERO }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.apply(&EditOp::AddOverlay {
        content: ClipContent::Text {
            text: "Mochi Studio — xuất thử".into(),
            font: "Segoe UI".into(),
            size_px: 64.0,
            color: "#FFFFFF".into(),
            background: Some("#1D1B26CC".into()),
        },
        start: Ticks::from_secs(0.5),
        duration: Ticks::from_secs(6.0),
        track_id: None,
    })
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.save(true).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
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

pub async fn run(av_only: bool) -> anyhow::Result<()> {
    let root = root();
    std::fs::create_dir_all(root.join("out"))?;
    if av_only {
        return av_sync(&root).await;
    }
    for e in std::fs::read_dir(root.join("out"))?.flatten() {
        let _ = std::fs::remove_file(e.path());
    }
    let out = |name: &str| root.join("out").join(name);

    println!("== K-X3: encoder máy hỗ trợ (mở thật + mã hóa thử) ==");
    let t0 = Instant::now();
    for c in encoder_capabilities() {
        println!(
            "  {:<5} → {:<12} {}  chạy được: {:?}{}",
            format!("{:?}", c.codec),
            c.encoder.clone().unwrap_or("(không)".into()),
            if c.hardware { "phần cứng" } else if c.encoder.is_some() { "phần mềm " } else { "         " },
            c.working,
            c.reason.as_ref().map(|r| format!("  — {r}")).unwrap_or_default()
        );
    }
    println!("  (kiểm mất {:.1}s)", t0.elapsed().as_secs_f64());

    let dir = root.join("Export.mochi");
    build_project(&dir)?;
    let (project, paths) = load(&dir)?;
    println!(
        "\n== Project thử: {} clip, canvas {}×{}, dài {:.2}s ==",
        project.clips.len(),
        project.canvas.width,
        project.canvas.height,
        mochi_export::settings::duration_of(&project).secs()
    );

    println!("\n== Preflight: từng luật ==");
    let mut s = settings(&out("Ban xuat.mp4"));
    match preflight(&project, &paths, &s) {
        Ok(e) => println!(
            "  bình thường → {}×{}, {} frame, ước tính {:.1} MB, encoder {:?}",
            e.width, e.height, e.frames, e.bytes as f64 / 1e6, e.encoder
        ),
        Err(e) => println!("  bình thường → LỖI {}", err(&e)),
    }
    std::fs::write(out("Ban xuat.mp4"), b"cu")?;
    println!("  K-X1 file đã có, overwrite=false → {}", preflight(&project, &paths, &s).err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    s.overwrite = true;
    println!("  K-X1 file đã có, overwrite=true → {}", preflight(&project, &paths, &s).map(|_| "OK".to_string()).unwrap_or_else(|e| err(&e)));
    std::fs::remove_file(out("Ban xuat.mp4"))?;
    s.overwrite = false;

    let mut missing = paths.clone();
    let music_id = project.media.iter().find(|m| m.name.ends_with(".mp3")).unwrap().id.clone();
    missing.remove(&music_id);
    println!("  K-X2 thiếu nhạc nền → {}", preflight(&project, &missing, &s).err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    s.allow_black_fill = true;
    println!(
        "  K-X2 thiếu nhạc nền + chấp nhận khung trống → {}",
        preflight(&project, &missing, &s).map(|e| format!("OK, cảnh báo {:?}", e.warnings)).unwrap_or_else(|e| err(&e))
    );
    s.allow_black_fill = false;

    for codec in [VideoCodec::Hevc, VideoCodec::Av1] {
        let mut c = s.clone();
        c.codec = codec;
        println!("  K-X3 chọn {codec:?} → {}", preflight(&project, &paths, &c).map(|e| format!("OK, encoder {:?}", e.encoder)).unwrap_or_else(|e| err(&e)));
    }
    let o = Command::new(std::env::current_exe()?)
        .args(["export", dir.to_str().unwrap(), "--out", out("av1.mp4").to_str().unwrap(), "--codec", "av1", "--range", "0,1"])
        .env("MOCHI_TEST_DISABLE_ENCODERS", "av1_nvenc,av1_qsv,av1_amf")
        .output()?;
    println!(
        "  K-X3 máy KHÔNG có encoder AV1 (giả lập bằng MOCHI_TEST_DISABLE_ENCODERS) → {}",
        String::from_utf8_lossy(&o.stdout).trim()
    );
    let mut c = s.clone();
    c.output = root.join("khong-ton-tai").join("x.mp4");
    println!("  thư mục đích không có / không ghi được → {}", preflight(&project, &paths, &c).err().map(|e| err(&e)).unwrap_or("OK?!".into()));
    let mut c = s.clone();
    c.output = out("Bản demo: v1?*.mp4");
    println!("  tên file có ký tự cấm → {}", preflight(&project, &paths, &c).map(|e| format!("{:?} → {}", e.warnings, e.output.file_name().unwrap().to_string_lossy())).unwrap_or_else(|e| err(&e)));
    let mut c = s.clone();
    c.format = ExportFormat::Gif;
    c.short_side = 1080;
    c.output = out("lon.gif");
    println!("  GIF 1080p30 cả project → {}", preflight(&project, &paths, &c).map(|e| format!("{:.0} MB, {:?}", e.bytes as f64 / 1e6, e.warnings)).unwrap_or_else(|e| err(&e)));
    let mut c = s.clone();
    c.range = range(5.0, 2.0);
    println!("  đoạn in > out → {}", preflight(&project, &paths, &c).err().map(|e| err(&e)).unwrap_or("OK?!".into()));

    println!("\n== X01 + X03: xuất thật qua tiến trình con ==");
    let mut a = settings(&out("Full 1080p.mp4"));
    let r = export_cmd::export(&project, &paths, &a, &[], |_| false);
    show("MP4 H.264 1080p30 cả project", &r);
    println!("      ffprobe: {}", ffprobe(&out("Full 1080p.mp4")));
    let full = out("Full 1080p.mp4");

    a.output = out("Doan 2-5s 720p.mp4");
    a.short_side = 720;
    a.range = range(2.0, 5.0);
    let r = export_cmd::export(&project, &paths, &a, &[], |_| false);
    show("MP4 đoạn 2–5 s, 720p", &r);
    println!("      ffprobe: {}", ffprobe(&a.output));

    for codec in [VideoCodec::Hevc, VideoCodec::Av1] {
        let mut c = settings(&out(&format!("{codec:?} 1080p.mp4")));
        c.codec = codec;
        c.range = range(0.0, 3.0);
        let r = export_cmd::export(&project, &paths, &c, &[], |_| false);
        show(&format!("MP4 {codec:?} 0–3 s"), &r);
        if r.result.is_ok() {
            println!("      ffprobe: {}", ffprobe(&c.output));
        }
    }

    let mut g = settings(&out("Doan 1-4s.gif"));
    g.format = ExportFormat::Gif;
    g.short_side = 480;
    g.fps = 15;
    g.quality = ExportQuality::Standard;
    g.range = range(1.0, 4.0);
    let r = export_cmd::export(&project, &paths, &g, &[], |_| false);
    show("X02 GIF 480p 15 fps, 1–4 s", &r);
    println!("      ffprobe: {}", ffprobe(&g.output));

    println!("\n== X04 + lỗi giữa chừng ==");
    let mut c = settings(&out("Huy.mp4"));
    let r = export_cmd::export(&project, &paths, &c, &[], |m| matches!(m, ChildMsg::Progress { frame, .. } if *frame >= 60));
    show("hủy ở ~frame 60", &r);
    println!("      còn file tạm: {}, còn file đích: {}", mochi_export::settings::partial_path(&c.output).exists(), c.output.exists());

    c.output = out("HW loi.mp4");
    c.range = range(0.0, 4.0);
    let r = export_cmd::export(&project, &paths, &c, &[("MOCHI_TEST_EXPORT_HW_FAIL_AT", "20")], |_| false);
    show("encoder phần cứng lỗi ở frame 20", &r);

    c.output = out("Het dia.mp4");
    let r = export_cmd::export(&project, &paths, &c, &[("MOCHI_TEST_EXPORT_FREE_BYTES", "30:10000000")], |_| false);
    show("hết dung lượng từ frame 30 (giả lập còn 10 MB)", &r);
    println!("      còn file tạm: {}, còn file đích: {}", mochi_export::settings::partial_path(&c.output).exists(), c.output.exists());

    c.output = out("Crash.mp4");
    let r = export_cmd::export(&project, &paths, &c, &[("MOCHI_TEST_EXPORT_CRASH_AT", "25")], |_| false);
    show("tiến trình con tự thoát đột ngột ở frame 25", &r);
    if let Err(ExportError::Crashed { log, .. }) = &r.result {
        println!("      log tồn tại: {} ({} dòng)", log.exists(), std::fs::read_to_string(log).map(|s| s.lines().count()).unwrap_or(0));
    }
    println!("      còn file tạm: {}, còn file đích: {}", mochi_export::settings::partial_path(&c.output).exists(), c.output.exists());

    c.output = out("Kill.mp4");
    unsafe { std::env::set_var("MOCHI_CLI_KILL_INSTEAD", "1") };
    let r = export_cmd::export(&project, &paths, &c, &[], |m| matches!(m, ChildMsg::Progress { frame, .. } if *frame >= 30));
    unsafe { std::env::remove_var("MOCHI_CLI_KILL_INSTEAD") };
    show("kill tiến trình con (Task Manager) ở ~frame 30", &r);
    println!("      còn file tạm: {}, còn file đích: {}", mochi_export::settings::partial_path(&c.output).exists(), c.output.exists());

    println!("\n== K-X4: hàng đợi theo project ==");
    let mut q: ExportQueue<&str> = ExportQueue::new();
    println!("  A1 (project A) → {:?}", q.push("A", "A1", "A1").0);
    println!("  A2 (project A) → {:?}", q.push("A", "A2", "A2").0);
    println!("  B1 (project B) → {:?}", q.push("B", "B1", "B1").0);
    println!("  A1 xong → chạy tiếp {:?}; B1 xong → {:?}", q.finish("A", "A1").map(|x| x.0), q.finish("B", "B1").map(|x| x.0));

    println!("\n== Preview = export (PSNR, frame render trực tiếp vs frame giải mã từ MP4 1080p) ==");
    crate::export_cmd::psnr_report(&project, &paths, &settings(&full), &full, &[0, 45, 84, 150, 239], &root.join("psnr")).await?;

    println!("\n== S05: đồng bộ tiếng–hình sau khi xuất ==");
    av_sync(&root).await?;
    Ok(())
}

/// Video 20 s: mỗi lần chớp trắng đi kèm một tiếng bíp cùng lúc (2, 7, 12, 17 s). Xuất ra MP4 rồi đo
/// lệch giữa frame trắng đầu tiên và mẫu tiếng đầu tiên của từng cặp.
pub async fn av_sync(root: &Path) -> anyhow::Result<()> {
    av_sync_long(root, 20, 5).await
}

/// S05: video chớp trắng + bíp cùng lúc mỗi `period` giây, dài `secs` giây → import → xuất → đo lệch
/// từng cặp chớp / bíp (cả ở phút cuối của bản dài).
pub async fn av_sync_long(root: &Path, secs: u32, period: u32) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    let src = root.join(format!("av-sync-src-{secs}s.mp4"));
    let tone = format!("aevalsrc=if(lt(mod(t-2\\,{period})\\,0.1)*gte(t\\,2)\\,0.8*sin(2*PI*1000*t)\\,0):s=48000:d={secs}");
    let flash = format!("drawbox=c=white:t=fill:enable=lt(mod(t-2\\,{period})\\,0.1)*gte(t\\,2)");
    let color = format!("color=c=black:s=640x360:r=30:d={secs}");
    ffmpeg(&[
        "-f", "lavfi", "-i", &color,
        "-f", "lavfi", "-i", &tone,
        "-vf", &flash,
        "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
        src.to_str().unwrap(),
    ]);
    let dir = root.join("AvSync.mochi");
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = EditorSession::create(&dir, "AvSync").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.import(&[src.clone()]);
    let mid = s.project().media[0].id.clone();
    s.apply(&EditOp::AddClip { media_id: mid, track_id: None, start: Ticks::ZERO }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.save(true).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    drop(s);
    let (project, paths) = load(&dir)?;
    let mut st = settings(&root.join("out").join("av-sync.mp4"));
    st.short_side = 360;
    st.overwrite = true;
    let r = export_cmd::export(&project, &paths, &st, &[], |_| false);
    show("xuất video chớp + bíp", &r);
    for (label, file) in [("nguồn", src.as_path()), ("bản xuất", st.output.as_path())] {
        let flashes = flash_times(file)?;
        let beeps = beep_times(file)?;
        let pairs: Vec<String> = flashes
            .iter()
            .zip(&beeps)
            .map(|(f, b)| format!("hình {f:.3}s / tiếng {b:.3}s (lệch {:+.0} ms)", (b - f) * 1000.0))
            .collect();
        let offsets: Vec<f64> = flashes.iter().zip(&beeps).map(|(f, b)| (b - f) * 1000.0).collect();
        let worst = offsets.iter().fold(0f64, |m, v| m.max(v.abs()));
        if pairs.len() > 6 {
            println!(
                "  {label}: {} cặp; đầu: {}; cuối: {}; lệch lớn nhất {worst:.0} ms",
                pairs.len(),
                pairs[..2].join("; "),
                pairs[pairs.len() - 2..].join("; ")
            );
        } else {
            println!("  {label}: {}", pairs.join("; "));
        }
    }
    println!("  hệ số bù trôi S05: tiếng 3600.9 s / hình 3600 s → ×{:.5}; tiếng 61 s / hình 60 s (thiết bị dừng muộn, không phải trôi) → ×{:.5}",
        mochi_preview::drift_rate(3600.9, 3600.0), mochi_preview::drift_rate(61.0, 60.0));
    Ok(())
}

/// Thời điểm (giây) các frame chuyển từ tối sang sáng.
fn flash_times(file: &Path) -> anyhow::Result<Vec<f64>> {
    let o = Command::new("ffmpeg")
        .args(["-v", "error", "-i"]).arg(file)
        .args(["-vf", "scale=8:8,format=gray", "-f", "rawvideo", "-"])
        .output()?;
    let fps = 30.0;
    let mut out = Vec::new();
    let mut prev = false;
    for (i, f) in o.stdout.chunks_exact(64).enumerate() {
        let bright = f.iter().map(|&v| v as u32).sum::<u32>() / 64 > 128;
        if bright && !prev {
            out.push(i as f64 / fps);
        }
        prev = bright;
    }
    Ok(out)
}

/// Thời điểm (giây) tiếng bíp bắt đầu (biên độ vượt ngưỡng sau một đoạn im).
fn beep_times(file: &Path) -> anyhow::Result<Vec<f64>> {
    let o = Command::new("ffmpeg")
        .args(["-v", "error", "-i"]).arg(file)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "48000", "-f", "s16le", "-"])
        .output()?;
    let samples: Vec<i16> = o.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let mut out = Vec::new();
    let mut quiet = 48_000usize;
    for (i, &v) in samples.iter().enumerate() {
        if v.unsigned_abs() > 8000 {
            if quiet > 24_000 {
                out.push(i as f64 / 48_000.0);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    Ok(out)
}
