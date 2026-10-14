//! Spike A: quay studio bằng pipeline của Cap (đã vendor).
//! Luồng tham khảo từ Cap `apps/cli/src/record.rs` (AGPL-3.0).

use cap_recording::{
    CameraFeed, MicrophoneFeed, RecordingDefaults,
    feeds::{camera, microphone},
    screen_capture::{self, ScreenCaptureTarget},
    studio_recording,
};
use kameo::Actor as _;
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(clap::Args)]
pub struct RecordArgs {
    /// Số giây quay
    #[arg(long, default_value_t = 5.0)]
    secs: f64,
    /// id màn hình (xem `mochi-cli targets`); mặc định màn hình đầu tiên
    #[arg(long)]
    display: Option<String>,
    /// id cửa sổ (ưu tiên hơn --display)
    #[arg(long)]
    window: Option<String>,
    /// Tên mic
    #[arg(long)]
    mic: Option<String>,
    /// id webcam, hoặc một phần tên webcam
    #[arg(long)]
    camera: Option<String>,
    /// Thu âm thanh hệ thống
    #[arg(long)]
    system_audio: bool,
    /// fps tối đa
    #[arg(long)]
    fps: Option<u32>,
    /// Thư mục bản quay sẽ tạo (không được tồn tại trước)
    #[arg(long)]
    out: Option<PathBuf>,
}

pub async fn run(args: RecordArgs) -> anyhow::Result<()> {
    let target = resolve_target(&args)?;
    let path = args.out.clone().unwrap_or_else(|| {
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        PathBuf::from(format!("recordings/rec-{id}"))
    });
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&path)
        .map_err(|e| anyhow::anyhow!("Không tạo được thư mục {}: {e}", path.display()))?;

    let mut builder = studio_recording::Actor::builder(path.clone(), target)
        .with_system_audio(args.system_audio);

    let mut camera_active = false;
    if let Some(device_id) = args.camera.as_deref() {
        let info = cap_camera::list_cameras()
            .find(|c| c.device_id() == device_id || c.display_name().contains(device_id))
            .ok_or_else(|| anyhow::anyhow!("Không thấy webcam id '{device_id}'"))?;
        let feed = CameraFeed::spawn(CameraFeed::default());
        feed.ask(camera::SetInput {
            id: camera::DeviceOrModelID::from_info(&info),
            settings: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("Đặt webcam lỗi: {e}"))?
        .await
        .map_err(|e| anyhow::anyhow!("Webcam không kết nối được: {e}"))?;
        let lock = feed
            .ask(camera::Lock)
            .await
            .map_err(|e| anyhow::anyhow!("Khóa webcam lỗi: {e}"))?;
        builder = builder.with_camera_feed(Arc::new(lock));
        camera_active = true;
    }

    if let Some(mic) = args.mic.as_deref() {
        if !MicrophoneFeed::list().contains_key(mic) {
            anyhow::bail!("Không thấy mic '{mic}'");
        }
        let (error_tx, _error_rx) = flume::bounded(16);
        let feed = MicrophoneFeed::spawn(MicrophoneFeed::new(error_tx));
        feed.ask(microphone::SetInput {
            label: mic.to_string(),
            settings: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("Đặt mic lỗi: {e}"))?
        .await
        .map_err(|e| anyhow::anyhow!("Mic không kết nối được: {e}"))?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let lock = feed
            .ask(microphone::Lock)
            .await
            .map_err(|e| anyhow::anyhow!("Khóa mic lỗi: {e}"))?;
        builder = builder.with_mic_feed(Arc::new(lock));
    }

    let builder =
        RecordingDefaults::default().apply_to_studio_builder(builder, camera_active, args.fps);
    let actor = builder
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("Không bắt đầu quay được: {e}"))?;

    println!("● Đang quay {} giây → {}", args.secs, path.display());
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs_f64(args.secs)) => {}
        _ = tokio::signal::ctrl_c() => println!("(Ctrl+C) dừng sớm"),
    }

    let completed = actor
        .stop()
        .await
        .map_err(|e| anyhow::anyhow!("Dừng quay lỗi: {e}"))?;
    let project_path = completed.project_path.clone();
    tokio::task::spawn_blocking(move || {
        cap_recording::recovery::RecoveryManager::remux_if_needed(&project_path)
    })
    .await?
    .map_err(|e| anyhow::anyhow!("Finalize lỗi: {e}"))?;

    println!("■ Xong: {}", completed.project_path.display());
    crate::inspect::run(&completed.project_path)
}

fn resolve_target(args: &RecordArgs) -> anyhow::Result<ScreenCaptureTarget> {
    if let Some(id) = &args.window {
        return screen_capture::list_windows()
            .into_iter()
            .find(|(w, _)| &w.id.to_string() == id)
            .map(|(w, _)| ScreenCaptureTarget::Window { id: w.id })
            .ok_or_else(|| anyhow::anyhow!("Không thấy cửa sổ id '{id}'"));
    }
    let displays = screen_capture::list_displays();
    let found = match &args.display {
        Some(id) => displays.into_iter().find(|(d, _)| &d.id.to_string() == id),
        None => displays.into_iter().next(),
    };
    found
        .map(|(d, _)| ScreenCaptureTarget::Display { id: d.id })
        .ok_or_else(|| anyhow::anyhow!("Không thấy màn hình phù hợp"))
}
