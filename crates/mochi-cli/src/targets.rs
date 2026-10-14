use cap_recording::{MicrophoneFeed, screen_capture};

pub fn run() -> anyhow::Result<()> {
    println!("== Màn hình ==");
    for (display, _) in screen_capture::list_displays() {
        println!(
            "  id={:<12} {}  ({} Hz)",
            display.id.to_string(),
            display.name,
            display.refresh_rate
        );
    }

    println!("\n== Cửa sổ ==");
    for (window, _) in screen_capture::list_windows() {
        println!(
            "  id={:<12} [{}] {}",
            window.id.to_string(),
            window.owner_name,
            truncate(&window.name, 60)
        );
    }

    println!("\n== Mic ==");
    for name in MicrophoneFeed::list().keys() {
        println!("  {name}");
    }

    println!("\n== Webcam ==");
    for camera in cap_camera::list_cameras() {
        println!("  id={}  {}", camera.device_id(), camera.display_name());
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
