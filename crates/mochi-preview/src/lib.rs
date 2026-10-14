//! mochi-preview — trình phát preview (E03, S06).
//!
//! Một thread riêng giữ `Renderer`, `Presenter` (swapchain của HWND con) và `AudioEngine`.
//! - Đang phát: đồng hồ = số mẫu âm thanh đã phát (không có loa thì dùng đồng hồ thật);
//!   mỗi vòng render frame tại thời điểm đó rồi present (Fifo → khóa theo vsync).
//! - Đang dừng: chỉ render khi có thay đổi (tua, sửa project), seek chính xác từng frame.

mod audio;
pub mod host;
mod present;

pub use audio::{AudioEngine, OfflineMixer, drift_rate, waveform};
pub use host::PhysRect;

use mochi_project::{Project, Ticks};
use mochi_render::{GpuContext, Renderer};
use present::Presenter;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

pub enum PlayerCommand {
    /// HWND con đã được tạo trên main thread; player tạo swapchain trên đó.
    Attach { hwnd: isize, width: u32, height: u32 },
    Resize { width: u32, height: u32 },
    /// Bỏ swapchain; trả lời khi xong để app hủy HWND an toàn.
    Detach { done: mpsc::Sender<()> },
    SetProject {
        project: Arc<Project>,
        paths: HashMap<String, PathBuf>,
        /// S03: proxy (media id → file) chỉ dùng để vẽ hình; tiếng vẫn đọc file gốc.
        proxies: HashMap<String, PathBuf>,
    },
    Play,
    Pause,
    TogglePlay,
    Seek(Ticks),
    /// Lùi/tiến n frame (theo fps của canvas), dừng phát.
    Step(i32),
    /// Bỏ project và mọi decoder (đóng file media); trả lời khi xong.
    Unload { done: mpsc::Sender<()> },
    /// X08: lưu khung tại đầu phát thành PNG, độ phân giải canvas, vẽ từ file gốc (không proxy).
    Snapshot { path: PathBuf, done: mpsc::Sender<Result<(u32, u32), String>> },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Time { t: Ticks, playing: bool },
    Stats { fps: f32, render_ms: f32, layers: u32 },
}

pub struct PreviewPlayer {
    tx: mpsc::Sender<PlayerCommand>,
}

impl PreviewPlayer {
    pub fn spawn(
        gpu: GpuContext,
        runtime: tokio::runtime::Handle,
        on_event: impl Fn(PlayerEvent) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("mochi-player".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || run(gpu, runtime, rx, on_event))
            .expect("không tạo được thread player");
        Self { tx }
    }

    pub fn send(&self, cmd: PlayerCommand) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for PreviewPlayer {
    fn drop(&mut self) {
        let _ = self.tx.send(PlayerCommand::Shutdown);
    }
}

fn duration_of(p: &Project) -> Ticks {
    p.clips.iter().map(|c| c.end()).max().unwrap_or(Ticks::ZERO)
}

fn run(
    gpu: GpuContext,
    rt: tokio::runtime::Handle,
    rx: mpsc::Receiver<PlayerCommand>,
    on_event: impl Fn(PlayerEvent),
) {
    let mut renderer = Renderer::new(gpu.clone());
    let audio = if mochi_project::test_env("MOCHI_PREVIEW_SILENT").is_some() {
        AudioEngine::silent()
    } else {
        AudioEngine::new()
    };
    let mut presenter: Option<Presenter> = None;
    let mut project: Option<Arc<Project>> = None;
    let mut paths: HashMap<String, PathBuf> = HashMap::new();
    // Đường dẫn để vẽ hình = file gốc, thay bằng proxy nếu có.
    let mut render_paths: HashMap<String, PathBuf> = HashMap::new();
    let mut playing = false;
    let mut t = Ticks::ZERO;
    let mut dirty = true;
    // Đồng hồ thật khi không có loa: (mốc thời gian, vị trí timeline lúc đó).
    let mut wall_anchor = (Instant::now(), Ticks::ZERO);
    let mut last_time_event = Instant::now() - Duration::from_secs(1);
    let mut fps_window = (Instant::now(), 0u32, Duration::ZERO);

    let set_playing = |on: bool, t: Ticks, anchor: &mut (Instant, Ticks)| {
        audio.seek(t);
        audio.set_playing(on);
        *anchor = (Instant::now(), t);
    };

    loop {
        // Đang dừng và không có gì đổi → chờ lệnh (không tốn CPU).
        let first = if playing || dirty {
            rx.try_recv().ok()
        } else {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        };
        let mut cmds: Vec<PlayerCommand> = first.into_iter().collect();
        cmds.extend(rx.try_iter());

        for cmd in cmds {
            match cmd {
                PlayerCommand::Attach { hwnd, width, height } => {
                    presenter = Presenter::new(&gpu, hwnd, width, height)
                        .map_err(|e| tracing::error!("preview surface: {e:#}"))
                        .ok();
                    dirty = true;
                }
                PlayerCommand::Resize { width, height } => {
                    if let Some(p) = presenter.as_mut() {
                        p.resize(&gpu, width, height);
                    }
                    dirty = true;
                }
                PlayerCommand::Detach { done } => {
                    presenter = None;
                    let _ = done.send(());
                }
                PlayerCommand::SetProject { project: p, paths: ps, proxies } => {
                    renderer.retain_media(&p);
                    audio.set_project(p.clone(), &ps);
                    t = t.min(duration_of(&p));
                    project = Some(p);
                    render_paths = ps.clone();
                    for (id, proxy) in proxies {
                        if render_paths.contains_key(&id) {
                            render_paths.insert(id, proxy);
                        }
                    }
                    paths = ps;
                    dirty = true;
                }
                PlayerCommand::Play | PlayerCommand::TogglePlay if !playing => {
                    let dur = project.as_deref().map(duration_of).unwrap_or_default();
                    if t >= dur {
                        t = Ticks::ZERO; // phát lại từ đầu khi đã ở cuối
                    }
                    playing = true;
                    set_playing(true, t, &mut wall_anchor);
                }
                PlayerCommand::Pause | PlayerCommand::TogglePlay => {
                    if playing {
                        playing = false;
                        set_playing(false, t, &mut wall_anchor);
                        dirty = true; // vẽ lại frame chính xác tại chỗ dừng
                    }
                }
                PlayerCommand::Play => {}
                PlayerCommand::Seek(to) => {
                    let dur = project.as_deref().map(duration_of).unwrap_or_default();
                    t = Ticks(to.0.clamp(0, dur.0));
                    set_playing(playing, t, &mut wall_anchor);
                    dirty = true;
                }
                PlayerCommand::Step(n) => {
                    let fps = project.as_ref().map_or(30, |p| p.canvas.fps.max(1)) as i64;
                    let dur = project.as_deref().map(duration_of).unwrap_or_default();
                    let frame = mochi_project::TICKS_PER_SECOND / fps;
                    t = Ticks((t.0 + n as i64 * frame).clamp(0, dur.0));
                    playing = false;
                    set_playing(false, t, &mut wall_anchor);
                    dirty = true;
                }
                PlayerCommand::Unload { done } => {
                    playing = false;
                    set_playing(false, Ticks::ZERO, &mut wall_anchor);
                    project = None;
                    paths.clear();
                    render_paths.clear();
                    t = Ticks::ZERO;
                    renderer.clear();
                    audio.clear();
                    let _ = done.send(());
                }
                PlayerCommand::Snapshot { path, done } => {
                    let r = match project.clone() {
                        None => Err("chưa mở project".to_string()),
                        Some(p) => {
                            let (tex, _) = rt.block_on(renderer.render(&p, &paths, t, true));
                            let (w, h) = (tex.width(), tex.height());
                            let rgba = mochi_render::read_texture_rgba(&gpu, &tex);
                            image::RgbaImage::from_raw(w, h, rgba)
                                .ok_or_else(|| "khung rỗng".to_string())
                                .and_then(|img| img.save(&path).map_err(|e| e.to_string()))
                                .map(|()| (w, h))
                        }
                    };
                    let _ = done.send(r);
                    dirty = true; // vẽ lại preview (bằng proxy nếu đang dùng)
                }
                PlayerCommand::Shutdown => return,
            }
        }

        let Some(p) = project.clone() else { continue };
        if playing {
            t = if audio.has_device() {
                audio.position()
            } else {
                wall_anchor.1 + Ticks::from_secs(wall_anchor.0.elapsed().as_secs_f64())
            };
            let dur = duration_of(&p);
            if t >= dur {
                t = dur;
                playing = false;
                set_playing(false, t, &mut wall_anchor);
            }
        }
        if !(playing || dirty) {
            continue;
        }
        dirty = false;

        let started = Instant::now();
        let (canvas, stats) = rt.block_on(renderer.render(&p, &render_paths, t, !playing));
        let render_time = started.elapsed();
        if render_time > Duration::from_millis(500) {
            tracing::info!("render chậm: {} ms (mở nguồn lần đầu?)", render_time.as_millis());
        }
        match presenter.as_mut() {
            Some(pr) => pr.present(&gpu, &canvas),
            None if playing => std::thread::sleep(Duration::from_millis(16)),
            None => {}
        }

        if last_time_event.elapsed() >= Duration::from_millis(33) || !playing {
            on_event(PlayerEvent::Time { t, playing });
            last_time_event = Instant::now();
        }
        fps_window.1 += 1;
        fps_window.2 += render_time;
        if fps_window.0.elapsed() >= Duration::from_secs(1) {
            let secs = fps_window.0.elapsed().as_secs_f32();
            on_event(PlayerEvent::Stats {
                fps: if playing { fps_window.1 as f32 / secs } else { 0.0 },
                render_ms: fps_window.2.as_secs_f32() * 1000.0 / fps_window.1.max(1) as f32,
                layers: stats.layers as u32,
            });
            fps_window = (Instant::now(), 0, Duration::ZERO);
        }
    }
}
