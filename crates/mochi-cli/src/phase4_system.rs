//! Phase 4 lượt 2, đợt C (xuất): gọi đúng tiến trình con xuất của app trên media TỔNG HỢP.
//! Ổ đầy giữa hai lần kiểm → báo hết dung lượng (không chuyển nhầm sang libx264) · encoder phần cứng lỗi
//! khi ổ còn trống → vẫn chuyển libx264 · hủy chậm bị kill → "đã hủy" · job.json / log được dọn.

use crate::export_cmd;
use mochi_editor::EditorSession;
use mochi_export::{ChildMsg, ExportError, ExportFormat};
use mochi_project::Ticks;
use mochi_timeline::EditOp;
use std::process::Command;

fn logs_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap().join("recordings/round4/logs")
}

fn count(ext: &str) -> usize {
    std::fs::read_dir(logs_dir())
        .map(|r| r.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(ext)).count())
        .unwrap_or(0)
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase4").join("system");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let src = root.join("src.mp4");
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=40", "-pix_fmt", "yuv420p"])
        .arg(&src)
        .status()?
        .success();
    anyhow::ensure!(ok, "ffmpeg");
    let dir = root.join("Sys.mochi");
    let mut s = EditorSession::create(&dir, "Sys").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let id = s.import(&[src]).imported[0].clone();
    s.apply(&EditOp::AddClip { media_id: id, track_id: None, start: Ticks::ZERO }).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    s.save(true).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    drop(s);
    let (project, paths) = export_cmd::load(&dir)?;
    let (jobs0, logs0) = (count(".job.json"), count(".log"));

    println!("== Ổ đầy giữa hai lần kiểm (frame 16, kiểm mỗi 15 frame) + lỗi ghi ==");
    let mut st = export_cmd::settings(&root.join("a.mp4"));
    st.short_side = 360;
    let r = export_cmd::export(&project, &paths, &st, &[("MOCHI_TEST_EXPORT_FREE_BYTES", "16:1000"), ("MOCHI_TEST_EXPORT_HW_FAIL_AT", "16")], |_| false);
    let fallback = r.msgs.iter().any(|m| m.contains("fallback"));
    println!(
        "  kết quả: {} · có chuyển sang libx264: {fallback} · file dở còn: {}",
        r.result.as_ref().err().map(export_cmd::err).unwrap_or_else(|| "XONG (sai)".into()),
        root.join("a.mp4").exists()
    );

    println!("\n== Đối chứng: encoder phần cứng lỗi khi ổ còn trống ==");
    let mut st = export_cmd::settings(&root.join("b.mp4"));
    st.short_side = 360;
    let r = export_cmd::export(&project, &paths, &st, &[("MOCHI_TEST_EXPORT_HW_FAIL_AT", "16")], |_| false);
    let fallback = r.msgs.iter().any(|m| m.contains("fallback"));
    println!(
        "  kết quả: {} · chuyển sang libx264: {fallback}",
        match &r.result {
            Ok(ChildMsg::Done { encoder, .. }) => format!("xong bằng {encoder}"),
            Ok(_) => "?".into(),
            Err(e) => export_cmd::err(e),
        }
    );

    println!("\n== Hủy mà tiến trình con không phản hồi (bị kill sau 10 s) ==");
    let mut st = export_cmd::settings(&root.join("c.gif"));
    st.format = ExportFormat::Gif;
    st.short_side = 360;
    st.fps = 15;
    let t0 = std::time::Instant::now();
    let r = export_cmd::export(&project, &paths, &st, &[("MOCHI_TEST_EXPORT_IGNORE_CANCEL", "1")], |m| matches!(m, ChildMsg::Started { .. }));
    println!(
        "  sau {:.1}s: {} (trước đây: \"crashed\") · file dở còn: {}",
        t0.elapsed().as_secs_f64(),
        match &r.result {
            Err(ExportError::Cancelled) => "đã hủy".to_string(),
            Err(e) => export_cmd::err(e),
            Ok(_) => "xong (không kịp hủy)".into(),
        },
        root.join("c.gif").exists()
    );

    println!("\n== job.json / log ==");
    println!(
        "  trong recordings/round4/logs: job.json {jobs0} → {}, log {logs0} → {} (giữ log của lần lỗi, tối đa {})",
        count(".job.json"),
        count(".log"),
        mochi_export::child::KEEP_LOGS
    );
    Ok(())
}
