//! R01 (v1): kiểm thử "bám theo cửa sổ". Tạo một cửa sổ thử 160×120 màu hồng (trên cùng), quay đúng
//! cửa sổ đó qua RecorderService, kéo nó sang phải trong lúc quay, rồi đo tỉ lệ điểm hồng trong từng
//! khung của bản quay: bám theo đúng → mọi khung ~100 % hồng. Chỉ đo số, không mở ảnh; bản quay bị
//! xóa sau khi đo (nếu bám theo hỏng thì bản quay có thể chứa một mẩu màn hình thật).

use mochi_recorder::{CaptureTarget, KeystrokeMode, RecorderService, RecordingConfig, RecordingQuality};
use std::{process::Command, time::Duration};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

/// Cửa sổ thử phải thuộc tiến trình KHÁC (Cap bỏ qua cửa sổ của chính tiến trình đang quay):
/// mở một form WinForms hồng, không viền, trên cùng bằng PowerShell rồi tìm HWND theo tiêu đề.
fn spawn_test_window() -> anyhow::Result<(isize, std::process::Child)> {
    let script = "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.Form;         $f.Text = 'Mochi follow test'; $f.FormBorderStyle = 'None'; $f.TopMost = $true;         $f.BackColor = [System.Drawing.Color]::FromArgb(255, 0, 255); $f.StartPosition = 'Manual';         $f.Location = New-Object System.Drawing.Point(0, 0); $f.Size = New-Object System.Drawing.Size(160, 120);         [System.Windows.Forms.Application]::Run($f)";
    let child = Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script]).spawn()?;
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(h) = unsafe { FindWindowW(None, w!("Mochi follow test")) }
            && !h.0.is_null()
        {
            return Ok((h.0 as isize, child));
        }
    }
    anyhow::bail!("không thấy cửa sổ thử")
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase6").join("follow");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let (hwnd, mut child) = spawn_test_window()?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let service = RecorderService::new();
    let config = RecordingConfig {
        target: CaptureTarget::Window { id: (hwnd as u64).to_string() },
        mic: None,
        camera: None,
        system_audio: false,
        fps: 30,
        quality: RecordingQuality::Standard,
        countdown_secs: 0,
        keystrokes: KeystrokeMode::Off,
        output_root: root.clone(),
        cursor_highlight: Default::default(),
        frame: true,
    };
    let paths = service.start(config, "Follow".into()).await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    // Kéo cửa sổ sang phải 400 px trong ~1 s.
    for i in 1..=20 {
        unsafe {
            let _ = SetWindowPos(HWND(hwnd as *mut _), Some(HWND_TOPMOST), i * 20, 0, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let rec = service.stop().await.map_err(|e| anyhow::anyhow!("{e:?}"))?;
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = rec;
    // Đo tỉ lệ điểm hồng của từng khung (thu nhỏ 16×12, 5 khung / giây).
    let display = walk(&paths.recording_dir).into_iter().find(|p| p.ends_with("display.mp4"));
    let Some(display) = display else { anyhow::bail!("không thấy display.mp4") };
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&display)
        .args(["-vf", "fps=5,scale=16:12", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()?;
    let frame = 16 * 12 * 3;
    let ratios: Vec<String> = out
        .stdout
        .chunks_exact(frame)
        .map(|f| {
            let pink = f.chunks_exact(3).filter(|p| p[0] > 200 && p[1] < 70 && p[2] > 200).count();
            format!("{:.0}%", pink as f64 / (16.0 * 12.0) * 100.0)
        })
        .collect();
    println!("  tỉ lệ điểm hồng từng khung (0,2 s / khung; cửa sổ bị kéo từ giây ~1,2 tới ~2,2): {}", ratios.join(" "));
    let _ = std::fs::remove_dir_all(&root);
    println!("  đã xóa bản quay thử");
    Ok(())
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                v.extend(walk(&p));
            } else {
                v.push(p);
            }
        }
    }
    v
}
