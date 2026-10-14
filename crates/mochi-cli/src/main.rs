//! mochi-cli — công cụ dev để kiểm thử thật lõi Mochi Studio.
//!
//! Không phải tính năng cho người dùng cuối (xem docs/plan-phase2.md mục 11).
//! Mọi lệnh in ra kết quả cụ thể để người xem tự đánh giá, không chỉ báo "pass".

mod device_scenario;
mod phase4_editor;
mod phase4_system;
mod phase5_highlight;
mod phase6_render;
mod phase6_recording;
mod follow_window;
mod phase6_audio;
mod phase6_captions;
mod editor_scenario;
mod export_cmd;
mod export_scenario;
mod jobs_scenario;
mod fake_recording;
mod polish_scenario;
mod inspect;
mod preview_bench;
mod record;
mod render_frame;
mod scenario;
mod sync_scenario;
mod targets;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "mochi-cli", about = "Mochi Studio dev CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Liệt kê màn hình, cửa sổ, mic, webcam có thể quay
    Targets,
    /// Quay studio (mỗi nguồn một track + dữ liệu con trỏ riêng) trong N giây
    Record(record::RecordArgs),
    /// In thông tin một thư mục bản quay: segment, stream, số sự kiện con trỏ
    Inspect { path: PathBuf },
    /// Render frame của project tại các mốc thời gian → PNG (Vòng 2, S06)
    RenderFrame {
        project: PathBuf,
        /// Các mốc (giây), cách nhau bởi dấu phẩy
        #[arg(long, value_delimiter = ',', default_value = "0.5")]
        at: Vec<f64>,
        #[arg(long, default_value = "recordings/frames")]
        out: PathBuf,
    },
    /// Tạo gói project chứa một bản quay TỔNG HỢP (layout Cap) để kiểm thử polish (Vòng 3)
    FakeRecording {
        #[arg(long, default_value = "recordings/round3/Fake.mochi")]
        out: PathBuf,
    },
    /// Vòng 3: polish (nền, zoom, con trỏ, webcam, chữ, làm mờ, khung, tách tiếng) trên bản quay tổng hợp
    Polish,
    /// Đo hiệu năng phát preview trên media tổng hợp (Vòng 2)
    PreviewBench {
        #[arg(long, default_value_t = 5.0)]
        secs: f64,
        /// Đo trên một project có sẵn (mặc định: tạo project video tổng hợp)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Làm ấm preview (Vòng 4): frame đầu nguội vs sau khi mở lại project; thả media rồi xóa
    WarmBench {
        #[arg(default_value = "recordings/round3/Fake.mochi")]
        project: PathBuf,
    },
    /// Vòng 4: encoder máy hỗ trợ
    ExportCaps,
    /// Vòng 4: xuất một project (preflight ở đây, render + encode ở tiến trình con)
    Export {
        project: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "mp4")]
        format: String,
        #[arg(long, default_value = "h264")]
        codec: String,
        #[arg(long, default_value_t = 1080)]
        short: u32,
        #[arg(long, default_value_t = 30)]
        fps: u32,
        /// Đoạn xuất, ví dụ `2,5` (giây)
        #[arg(long)]
        range: Option<String>,
        #[arg(long)]
        overwrite: bool,
    },
    /// Vòng 4: kịch bản xuất (K-X1…K-X4, lỗi giữa chừng, PSNR, A/V sync) trên media tổng hợp
    ExportScenario {
        /// Chỉ chạy phần đo đồng bộ tiếng–hình (S05)
        #[arg(long)]
        av_only: bool,
        /// Độ dài video chớp + bíp (giây) cho phần đo đồng bộ; mặc định 20 (bíp mỗi 5 s), dài hơn thì bíp mỗi 60 s
        #[arg(long)]
        av_secs: Option<u32>,
    },
    /// Vòng 5: proxy + tạm hoãn, tốc độ preview, cache (K-S3), autosave (S01), kiểm GPU
    JobsScenario,
    /// Phase 4 lượt 2 đợt B: âm lượng track, kẹp giá trị, backup, khôi phục, relink, GIF động, proxy 4K, webcam tự do
    Phase4Editor,
    /// Phase 4 lượt 2 đợt C: ổ đầy giữa chừng khi xuất, hủy bị kill, dọn job.json / log
    Phase4System,
    /// Phase 5: highlight con trỏ (R13 / E06) trên bản quay tổng hợp
    Phase5Highlight,
    /// Phase 6 lượt B: compositor v1 (keyframe, chuyển cảnh, khung thiết bị, hình khối, callout, spotlight)
    Phase6Render,
    /// Phase 6 lượt C: hiệu ứng bản quay (spotlight / kính lúp / kiểu con trỏ / tiếng click, webcam, phím bấm)
    Phase6Recording,
    /// R01 v1: quay một cửa sổ thử rồi kéo nó đi — bản quay phải bám theo
    FollowWindow,
    /// Phase 6 lượt D: khử ồn, chuẩn hóa, nén, ducking trên âm thanh tổng hợp
    Phase6Audio,
    /// Phase 6 lượt E: phụ đề (SRT, burn-in, phụ đề động, xuất kèm .srt)
    Phase6Captions,
    /// Phase 4: quay vùng 64×64 + webcam + mic (Tiêu chuẩn, 60 fps); người dùng rút thiết bị giữa chừng (R06, R07)
    DeviceLoss {
        #[arg(long, default_value_t = 25)]
        secs: u64,
        /// Một phần tên webcam
        #[arg(long, default_value = "1080P Web Camera")]
        camera: String,
        /// Một phần tên mic
        #[arg(long, default_value = "1080P Web Camera")]
        mic: String,
        /// Tạm dừng 2 s ở giây thứ N rồi tiếp tục (thử Resume sau khi mất thiết bị)
        #[arg(long)]
        pause_at: Option<u64>,
        #[arg(long)]
        keep: bool,
    },
    /// Kịch bản kiểm thử thật của RecorderService (Vòng 1)
    #[command(subcommand)]
    Scenario(scenario::Scenario),
}

/// Pipeline WGPU/MediaFoundation/FFmpeg của Cap có stack rất sâu (nhất là bản debug):
/// runtime và worker tokio chạy trên stack 16 MB như Cap CLI.
const RUNTIME_STACK_SIZE: usize = 16 * 1024 * 1024;

fn main() -> anyhow::Result<()> {
    // Toạ độ cửa sổ/màn hình/con trỏ phải theo pixel vật lý (giống Cap CLI).
    #[cfg(windows)]
    {
        use windows::Win32::UI::HiDpi::{PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness};
        let _ = unsafe { SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE) };
    }

    // Tiến trình con xuất (mochi-cli tự gọi lại chính nó, giống app).
    if let Some(job) = mochi_export::job_arg() {
        let code = std::thread::Builder::new()
            .stack_size(RUNTIME_STACK_SIZE)
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_stack_size(RUNTIME_STACK_SIZE)
                    .build()
                    .map(|rt| rt.block_on(mochi_export::child_main(&job)))
                    .unwrap_or(1)
            })?
            .join()
            .unwrap_or(1);
        std::process::exit(code);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let cli = Cli::parse();
    std::thread::Builder::new()
        .name("mochi-cli-runtime".into())
        .stack_size(RUNTIME_STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(RUNTIME_STACK_SIZE)
                .build()?
                .block_on(run(cli))
        })?
        .join()
        .map_err(|_| anyhow::anyhow!("runtime thread panicked"))?
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Targets => targets::run(),
        Command::Record(args) => record::run(args).await,
        Command::Inspect { path } => inspect::run(&path),
        Command::Scenario(s) => scenario::run(s).await,
        Command::Polish => polish_scenario::run().await,
        Command::FakeRecording { out } => {
            let _ = std::fs::remove_dir_all(&out);
            let arrow = std::fs::read_dir("recordings/round1")?
                .flatten()
                .flat_map(|p| std::fs::read_dir(p.path().join("recordings")).into_iter().flatten().flatten())
                .map(|r| r.path().join("content/cursors/cursor_0.png"))
                .find(|p| p.is_file())
                .ok_or_else(|| anyhow::anyhow!("cần ảnh con trỏ từ một bản quay Vòng 1"))?;
            let rec = fake_recording::make(&out, &arrow)?;
            println!("bản quay tổng hợp: {}", rec.display());
            Ok(())
        }
        Command::PreviewBench { secs, project } => preview_bench::run(secs, project).await,
        Command::WarmBench { project } => preview_bench::warm(project).await,
        Command::JobsScenario => jobs_scenario::run().await,
        Command::Phase4Editor => phase4_editor::run().await,
        Command::Phase4System => phase4_system::run().await,
        Command::Phase5Highlight => phase5_highlight::run().await,
        Command::Phase6Render => phase6_render::run().await,
        Command::Phase6Recording => phase6_recording::run().await,
        Command::FollowWindow => follow_window::run().await,
        Command::Phase6Audio => phase6_audio::run().await,
        Command::Phase6Captions => phase6_captions::run().await,
        Command::DeviceLoss { secs, camera, mic, pause_at, keep } => {
            device_scenario::run(secs, &camera, &mic, pause_at, keep).await
        }
        Command::ExportCaps => {
            for c in mochi_export::encoder_capabilities() {
                println!("{}", serde_json::to_string(c)?);
            }
            Ok(())
        }
        Command::ExportScenario { av_only, av_secs } => match av_secs {
            Some(secs) => export_scenario::av_sync_long(&export_scenario::root(), secs, if secs > 60 { 60 } else { 5 }).await,
            None => export_scenario::run(av_only).await,
        },
        Command::Export { project, out, format, codec, short, fps, range, overwrite } => {
            let (p, paths) = export_cmd::load(&project)?;
            let mut s = export_cmd::settings(&out);
            s.format = match format.as_str() {
                "gif" => mochi_export::ExportFormat::Gif,
                "m4a" => mochi_export::ExportFormat::M4a,
                "mp3" => mochi_export::ExportFormat::Mp3,
                "webm" => mochi_export::ExportFormat::Webm,
                _ => mochi_export::ExportFormat::Mp4,
            };
            s.codec = match codec.as_str() {
                "hevc" => mochi_export::VideoCodec::Hevc,
                "av1" => mochi_export::VideoCodec::Av1,
                _ => mochi_export::VideoCodec::H264,
            };
            s.short_side = short;
            s.fps = fps;
            s.overwrite = overwrite;
            if let Some(r) = range {
                let (a, b) = r.split_once(',').ok_or_else(|| anyhow::anyhow!("--range a,b"))?;
                s.range = export_cmd::range(a.trim().parse()?, b.trim().parse()?);
            }
            let r = export_cmd::export(&p, &paths, &s, &[], |m| {
                if let mochi_export::ChildMsg::Progress { frame, frames, fps, eta_secs } = m {
                    eprint!("\r  {frame}/{frames}  {fps:.1} fps  còn {eta_secs:.0}s   ");
                }
                false
            });
            eprintln!();
            match r.result {
                Ok(done) => println!("{}", serde_json::to_string(&done)?),
                Err(e) => println!("LỖI {}", export_cmd::err(&e)),
            }
            Ok(())
        }
        Command::RenderFrame { project, at, out } => render_frame::run(project, at, out).await,
    }
}
