//! Kịch bản kiểm thử thật cho Vòng 1 (Người quay): gọi đúng `RecorderService` mà app dùng
//! và IN RA kết quả cụ thể (sự kiện, file, phím đã ghi) để người xem tự đánh giá.

use mochi_recorder::{
    CaptureTarget, KeystrokeMode, RecorderEvent, RecorderService, RecordingConfig,
    RecordingQuality, recovery, targets,
};
use std::{path::PathBuf, time::Duration};

#[derive(clap::Subcommand)]
pub enum Scenario {
    /// Đếm ngược 2s → quay 3s → tạm dừng 2s → quay tiếp 3s → dừng; giả lập phím F13–F15 (K-R3)
    Basic {
        #[arg(long, default_value = "modifiers-only")]
        keystrokes: String,
        #[arg(long)]
        mic: Option<String>,
        #[arg(long)]
        system_audio: bool,
    },
    /// K-R1 (2 phiên), K-R4 (ổ đầy), hủy (xóa thư mục), bản quá ngắn
    Rules,
    /// Quay N giây rồi dừng (dùng cho thử crash: kill process giữa chừng)
    Long {
        #[arg(long, default_value_t = 30)]
        secs: u64,
    },
    /// Vòng 5 / S05: quay dài (vùng 64×64 + âm thanh hệ thống), đo lệch tiếng–hình, xuất và đo lại
    Sync {
        #[arg(long, default_value_t = 3600)]
        secs: u64,
        /// Giữ bản quay + file xuất (mặc định xóa)
        #[arg(long)]
        keep: bool,
    },
    /// Vòng 2: phiên biên tập (mở, import, EditOp, undo, khóa, backup, relink…)
    Editor,
    /// (nội bộ) mở một project rồi thoát — process thứ hai cho kiểm thử K-E3
    EditorOpen { dir: PathBuf },
    /// (nội bộ) mở một project, giữ 60 giây — để thử crash (kill giữa chừng)
    EditorHold { dir: PathBuf },
    /// Liệt kê / khôi phục / bỏ bản quay dở
    Recover {
        #[arg(long)]
        restore: bool,
        #[arg(long)]
        discard: bool,
    },
}

fn root() -> PathBuf {
    std::env::current_dir().unwrap().join("recordings").join("round1")
}

fn config(keystrokes: KeystrokeMode, countdown: u32) -> RecordingConfig {
    let display = targets::list_displays().into_iter().next().expect("không có màn hình");
    RecordingConfig {
        target: CaptureTarget::Display { id: display.id },
        mic: None,
        camera: None,
        system_audio: false,
        fps: 30,
        quality: RecordingQuality::High,
        countdown_secs: countdown,
        keystrokes,
        output_root: root(), cursor_highlight: Default::default(),
    }
}

/// In mọi sự kiện của service kèm mốc thời gian.
fn print_events(service: &RecorderService) -> tokio::task::JoinHandle<()> {
    let mut rx = service.subscribe();
    let t0 = std::time::Instant::now();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            let line = match &ev {
                RecorderEvent::Stats { stats } => format!(
                    "stats  elapsed={:.1}s bytes={:.1}MB segments={} free={:.1}GB",
                    stats.elapsed_ms / 1000.0,
                    stats.bytes / 1e6,
                    stats.segments,
                    stats.free_bytes / 1e9
                ),
                other => serde_json::to_string(other).unwrap(),
            };
            println!("  [{:>5.2}s] {line}", t0.elapsed().as_secs_f64());
        }
    })
}

pub async fn run(scenario: Scenario) -> anyhow::Result<()> {
    std::fs::create_dir_all(root())?;
    match scenario {
        Scenario::Basic {
            keystrokes,
            mic,
            system_audio,
        } => basic(&keystrokes, mic, system_audio).await,
        Scenario::Rules => rules().await,
        Scenario::Long { secs } => long(secs).await,
        Scenario::Sync { secs, keep } => crate::sync_scenario::run(secs, keep).await,
        Scenario::Recover { restore, discard } => recover(restore, discard),
        Scenario::Editor => crate::editor_scenario::run(),
        Scenario::EditorHold { dir } => {
            let _s = mochi_editor::EditorSession::open(&dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            println!("đang giữ project");
            std::thread::sleep(Duration::from_secs(60));
            Ok(())
        }
        Scenario::EditorOpen { dir } => {
            crate::editor_scenario::open_only(&dir);
            Ok(())
        }
    }
}

async fn basic(keystrokes: &str, mic: Option<String>, system_audio: bool) -> anyhow::Result<()> {
    let mode = match keystrokes {
        "off" => KeystrokeMode::Off,
        "all" => KeystrokeMode::All,
        _ => KeystrokeMode::ModifiersOnly,
    };
    let service = RecorderService::new();
    let printer = print_events(&service);
    let mut cfg = config(mode, 2);
    cfg.mic = mic;
    cfg.system_audio = system_audio;

    println!("== start (keystrokes={mode:?}) ==");
    let paths = service.start(cfg, "Kịch bản basic".into()).await?;
    println!("  project: {}", paths.project_dir.display());
    tokio::time::sleep(Duration::from_millis(2600)).await; // hết đếm ngược

    println!("== giả lập phím: Ctrl+F13 (giữ), F14 (bỏ), Shift+F15 (bỏ), Alt+F15 (giữ) ==");
    keys::combo(&[keys::VK_CONTROL], keys::VK_F13);
    keys::combo(&[], keys::VK_F14);
    keys::combo(&[keys::VK_SHIFT], keys::VK_F15);
    keys::combo(&[keys::VK_MENU], keys::VK_F15);
    tokio::time::sleep(Duration::from_millis(2500)).await;

    println!("== pause ==");
    service.pause().await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("== resume ==");
    service.resume().await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    println!("== stop ==");
    let finished = service.stop().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    printer.abort();

    println!(
        "\n== kết quả: \"{}\" {:.2}s, {} segment, quá ngắn={} ==",
        finished.name,
        finished.duration_ms / 1000.0,
        finished.segments,
        finished.too_short
    );
    crate::inspect::run(&finished.recording_dir)?;
    print_keyboard(&finished.recording_dir);
    let leftovers: Vec<_> = std::fs::read_dir(&finished.recording_dir)?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".recovery-"))
        .collect();
    println!("\n== thư mục .recovery-* còn lại: {} ==", leftovers.len());
    Ok(())
}

fn print_keyboard(recording_dir: &std::path::Path) {
    println!("\n== Phím đã ghi (keyboard.bin) ==");
    let segments = recording_dir.join("content").join("segments");
    let Ok(entries) = std::fs::read_dir(&segments) else {
        return;
    };
    let mut any = false;
    for seg in entries.flatten() {
        let path = seg.path().join(cap_project::KEYBOARD_EVENTS_FILE_NAME);
        if !path.exists() {
            println!("  {}: (không có file)", seg.file_name().to_string_lossy());
            continue;
        }
        match cap_project::KeyboardEvents::load_from_file(&path) {
            Ok(ev) => {
                any = true;
                let keys: Vec<String> = ev
                    .presses
                    .iter()
                    .map(|p| format!("{}{}", if p.down { "↓" } else { "↑" }, p.key))
                    .collect();
                println!(
                    "  {}: {}",
                    seg.file_name().to_string_lossy(),
                    if keys.is_empty() { "(trống)".into() } else { keys.join(" ") }
                );
            }
            Err(e) => println!("  lỗi đọc: {e}"),
        }
    }
    if !any {
        println!("  (không segment nào có keyboard.bin)");
    }
}

async fn rules() -> anyhow::Result<()> {
    let service = RecorderService::new();

    println!("== K-R1: bắt đầu phiên thứ hai khi đang quay ==");
    let p = service.start(config(KeystrokeMode::Off, 0), "Kịch bản K-R1".into()).await?;
    let second = service.start(config(KeystrokeMode::Off, 0), "Phiên thứ hai".into()).await;
    println!("  phiên 2 → {}", serde_json::to_string(&second.err())?);
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("== Hủy: thư mục phải bị xóa ==");
    service.cancel().await?;
    println!("  {} còn tồn tại? {}", p.project_dir.display(), p.project_dir.exists());

    println!("== K-R4: ngưỡng trống tối thiểu = 1 PB (giả lập ổ đầy) ==");
    unsafe { std::env::set_var("MOCHI_TEST_MIN_FREE_BYTES", "1000000000000000") };
    let low = service.start(config(KeystrokeMode::Off, 0), "Kịch bản K-R4".into()).await;
    println!("  start → {}", serde_json::to_string(&low.err())?);
    unsafe { std::env::remove_var("MOCHI_TEST_MIN_FREE_BYTES") };
    println!("  trạng thái sau đó: {:?}", service.state().await);

    println!("== Bản quá ngắn (0,4 giây) ==");
    service.start(config(KeystrokeMode::Off, 0), "Kịch bản ngắn".into()).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let short = service.stop().await?;
    println!(
        "  thời lượng {:.2}s, quá ngắn = {}",
        short.duration_ms / 1000.0,
        short.too_short
    );
    service.discard_finished(&root(), &short.project_dir).await?;
    println!("  đã bỏ → còn tồn tại? {}", short.project_dir.exists());

    println!("== Ổ đầy khi đang quay: ngưỡng dừng = 1 PB → phải tự dừng và GIỮ phần đã quay ==");
    let printer = print_events(&service);
    service.start(config(KeystrokeMode::Off, 0), "Kịch bản ổ đầy".into()).await?;
    unsafe { std::env::set_var("MOCHI_TEST_MIN_FREE_WHILE_RECORDING_BYTES", "1000000000000000") };
    tokio::time::sleep(Duration::from_secs(4)).await;
    unsafe { std::env::remove_var("MOCHI_TEST_MIN_FREE_WHILE_RECORDING_BYTES") };
    printer.abort();
    println!("  trạng thái sau đó: {:?}", service.state().await);

    println!("== K-R2: không có lệnh đổi thiết bị; mute mic khi không có mic ==");
    service.start(config(KeystrokeMode::Off, 0), "Kịch bản K-R2".into()).await?;
    let mute = service.set_mic_muted(true).await;
    println!("  set_mic_muted → {}", serde_json::to_string(&mute.err())?);
    service.cancel().await?;
    Ok(())
}

async fn long(secs: u64) -> anyhow::Result<()> {
    let service = RecorderService::new();
    let printer = print_events(&service);
    let paths = service.start(config(KeystrokeMode::Off, 0), "Kịch bản crash".into()).await?;
    println!("  recording: {}", paths.recording_dir.display());
    tokio::time::sleep(Duration::from_secs(secs)).await;
    let f = service.stop().await?;
    printer.abort();
    println!("  xong {:.1}s", f.duration_ms / 1000.0);
    Ok(())
}

fn recover(restore: bool, discard: bool) -> anyhow::Result<()> {
    let list = recovery::list(&root());
    println!("== Bản quay dở tìm thấy: {} ==", list.len());
    for r in &list {
        println!(
            "  {} — {} segment, ~{:.1}s — {}",
            r.name,
            r.segments,
            r.estimated_duration_ms / 1000.0,
            r.recording_dir.display()
        );
        if restore {
            recovery::restore(&r.recording_dir)?;
            println!("  → đã khôi phục");
            crate::inspect::run(&r.recording_dir)?;
        } else if discard {
            recovery::discard(&root(), &r.recording_dir)?;
            println!("  → đã bỏ");
        }
    }
    Ok(())
}

/// Giả lập phím bằng SendInput. Chỉ dùng F13–F15 (không app nào gán) để không ảnh hưởng
/// cửa sổ đang mở của người dùng.
mod keys {
    pub const VK_CONTROL: u16 = 0x11;
    pub const VK_SHIFT: u16 = 0x10;
    pub const VK_MENU: u16 = 0x12;
    pub const VK_F13: u16 = 0x7C;
    pub const VK_F14: u16 = 0x7D;
    pub const VK_F15: u16 = 0x7E;

    pub fn combo(mods: &[u16], key: u16) {
        for m in mods {
            send(*m, false);
        }
        sleep(80);
        send(key, false);
        sleep(80);
        send(key, true);
        sleep(80);
        for m in mods.iter().rev() {
            send(*m, true);
        }
        sleep(150);
    }

    fn sleep(ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    #[cfg(windows)]
    fn send(vk: u16, up: bool) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
            SendInput, VIRTUAL_KEY,
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    ..Default::default()
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    #[cfg(not(windows))]
    fn send(_vk: u16, _up: bool) {}
}
