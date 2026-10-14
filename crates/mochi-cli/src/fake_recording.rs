//! Bản quay TỔNG HỢP theo đúng layout của Cap — để kiểm thử polish / zoom / con trỏ / webcam
//! mà không động tới bản quay thật (màn hình riêng) của người dùng.
//!
//! - display.mp4: testsrc2 1920×1080 30 fps, 8 giây
//! - camera.mp4: SMPTE bars 640×480
//! - cursor.json: đường đi dựng sẵn + click ở 2,0 s và 2,4 s (cùng một chỗ) và 6,0 s (chỗ khác)

use serde_json::json;
use std::{path::Path, process::Command};

const SECS: f64 = 8.0;

fn ffmpeg(args: &[&str]) -> anyhow::Result<()> {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status()?.success();
    anyhow::ensure!(ok, "ffmpeg {args:?} thất bại");
    Ok(())
}

/// Vị trí con trỏ (0–1) tại thời điểm t: đi tới điểm click 1, đứng yên, rồi sang điểm click 2.
fn cursor_at(t: f64) -> (f64, f64) {
    let lerp = |a: (f64, f64), b: (f64, f64), k: f64| {
        let k = k.clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k)
    };
    let p0 = (0.25, 0.30);
    let p1 = (0.72, 0.62);
    let p2 = (0.30, 0.78);
    if t < 1.8 {
        lerp(p0, p1, t / 1.8)
    } else if t < 4.5 {
        p1
    } else if t < 5.8 {
        lerp(p1, p2, (t - 4.5) / 1.3)
    } else {
        p2
    }
}

pub fn make(project_dir: &Path, arrow_png: &Path) -> anyhow::Result<std::path::PathBuf> {
    make_sized(project_dir, arrow_png, 1920, 1080)
}

/// Như `make` với cỡ màn hình tùy chọn (Phase 4: bản quay 4K để thử proxy).
pub fn make_sized(project_dir: &Path, arrow_png: &Path, w: u32, h: u32) -> anyhow::Result<std::path::PathBuf> {
    let rec = project_dir.join("recordings").join("fake0001");
    let seg = rec.join("content").join("segments").join("segment-0");
    std::fs::create_dir_all(&seg)?;
    std::fs::create_dir_all(rec.join("content").join("cursors"))?;
    let secs = SECS.to_string();
    ffmpeg(&[
        "-f", "lavfi", "-i", &format!("testsrc2=size={w}x{h}:rate=30:duration={secs}"),
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "30",
        seg.join("display.mp4").to_str().unwrap(),
    ])?;
    ffmpeg(&[
        "-f", "lavfi", "-i", &format!("smptebars=size=640x480:rate=30:duration={secs}"),
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "30",
        seg.join("camera.mp4").to_str().unwrap(),
    ])?;
    std::fs::copy(arrow_png, rec.join("content").join("cursors").join("cursor_0.png"))?;

    let mut moves = vec![];
    let mut t = 0.0;
    while t < SECS {
        let (x, y) = cursor_at(t);
        moves.push(json!({"active_modifiers": [], "cursor_id": "0", "time_ms": t * 1000.0, "x": x, "y": y}));
        t += 1.0 / 30.0;
    }
    let mut clicks = vec![];
    for at in [2.0, 2.4, 6.0] {
        for (dt, down) in [(0.0, true), (0.1, false)] {
            clicks.push(json!({"active_modifiers": [], "cursor_num": 1, "cursor_id": "0", "time_ms": (at + dt) * 1000.0, "down": down}));
        }
    }
    std::fs::write(seg.join("cursor.json"), serde_json::to_vec_pretty(&json!({"clicks": clicks, "moves": moves}))?)?;

    let meta = json!({
        "platform": "Windows",
        "pretty_name": "Bản quay tổng hợp",
        "sharing": null,
        "segments": [{
            "display": {"path": "content/segments/segment-0/display.mp4", "fps": 30, "start_time": 0.0},
            "camera": {"path": "content/segments/segment-0/camera.mp4", "fps": 30, "start_time": 0.0},
            "cursor": "content/segments/segment-0/cursor.json"
        }],
        "cursors": {"0": {"imagePath": "content/cursors/cursor_0.png", "hotspot": {"x": 0.0625, "y": 0.0}, "shape": "Windows|Arrow"}},
        "status": {"status": "Complete"}
    });
    std::fs::write(rec.join("recording-meta.json"), serde_json::to_vec_pretty(&meta)?)?;
    // Phase 4 (R07 / R03): giả lập webcam mất ở 4,6–6,2 s và hai lần đổi cửa sổ active.
    use mochi_project::sidecar::{ActiveWindow, LostRange, LostSource, RecordingSidecar};
    RecordingSidecar {
        lost: vec![LostRange { source: LostSource::Camera, start: 4.6, end: 6.2 }],
        active_windows: vec![
            ActiveWindow { at: 0.0, app: "notepad.exe".into(), title: "Ghi chú".into() },
            ActiveWindow { at: 3.0, app: "msedge.exe".into(), title: "Trang web".into() },
        ],
        cursor_highlight: None,
    }
    .write(&rec)?;
    Ok(rec)
}
