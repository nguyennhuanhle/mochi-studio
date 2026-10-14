//! RecorderService: máy trạng thái của Người quay.
//!
//! Idle → Preparing → Countdown → Recording ⇄ Paused → Finalizing → Idle
//!
//! Luật thực thi ở đây (docs/plan-phase2.md mục 6):
//! - K-R1: `start` khi không ở Idle → `AlreadyRecording`.
//! - K-R2: khi đang quay chỉ nhận pause/resume/stop/cancel/mute; không có lệnh đổi cấu hình.
//! - K-R3: bộ lọc phím đặt theo `KeystrokeMode` trước mỗi phiên; Off = không thu phím.
//! - K-R4: preflight dung lượng ổ; đang quay mà ổ cạn thì tự dừng an toàn.

use crate::{
    CaptureTarget, DeviceKind, KeystrokeMode, RecorderError, RecordingConfig, disk,
    events::*,
    targets::resolve_target,
};
use cap_recording::{
    CameraFeed, MicrophoneFeed, RecordingDefaults, RecordingStartGate,
    cursor::{KeyboardCaptureFilter, set_keyboard_capture_filter},
    feeds::{camera, microphone},
    PipelineHealthEvent,
    studio_recording::{self, set_studio_health_sender},
};
use kameo::Actor as _;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{info, warn};

/// Một bản quay ngắn hơn ngưỡng này → hỏi người dùng giữ hay bỏ.
const MIN_KEEP_DURATION: Duration = Duration::from_secs(1);
const MONITOR_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct RecorderService {
    inner: Arc<Inner>,
}

struct Inner {
    phase: Mutex<Phase>,
    events: broadcast::Sender<RecorderEvent>,
}

enum Phase {
    Idle,
    /// Preparing hoặc Finalizing: đang bận, chưa có/không còn session.
    Busy(RecorderState),
    Active(Box<Session>),
}

struct Session {
    config: RecordingConfig,
    project_dir: PathBuf,
    recording_dir: PathBuf,
    name: String,
    actor: studio_recording::ActorHandle,
    gate: RecordingStartGate,
    mic_lock: Option<Arc<microphone::MicrophoneFeedLock>>,
    _camera_lock: Option<Arc<camera::CameraFeedLock>>,
    state: RecorderState,
    clock: Clock,
    segments: u32,
    mic_muted: bool,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// R03 + R07: cửa sổ active, khoảng mất thiết bị → `mochi-recording.json`.
    activity: crate::activity::ActivityLog,
}

#[derive(Default)]
struct Clock {
    started: Option<Instant>,
    paused_total: Duration,
    paused_since: Option<Instant>,
}

impl Clock {
    fn elapsed(&self) -> Duration {
        let Some(started) = self.started else {
            return Duration::ZERO;
        };
        let paused_now = self.paused_since.map_or(Duration::ZERO, |p| p.elapsed());
        started.elapsed().saturating_sub(self.paused_total + paused_now)
    }
}

impl Default for RecorderService {
    fn default() -> Self {
        Self::new()
    }
}

impl RecorderService {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                phase: Mutex::new(Phase::Idle),
                events,
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RecorderEvent> {
        self.inner.events.subscribe()
    }

    fn emit(&self, event: RecorderEvent) {
        let _ = self.inner.events.send(event);
    }

    fn emit_state(&self, state: RecorderState) {
        self.emit(RecorderEvent::StateChanged { state });
    }

    pub async fn state(&self) -> RecorderState {
        match &*self.inner.phase.lock().await {
            Phase::Idle => RecorderState::Idle,
            Phase::Busy(s) => *s,
            Phase::Active(s) => s.state,
        }
    }

    /// Bắt đầu một phiên quay. Trả về khi pipeline đã sẵn sàng; đếm ngược chạy nền,
    /// hết đếm ngược mới thật sự ghi (RecordingStartGate).
    pub async fn start(&self, config: RecordingConfig, name: String) -> Result<FinishedPaths, RecorderError> {
        // K-R1
        {
            let mut phase = self.inner.phase.lock().await;
            if !matches!(*phase, Phase::Idle) {
                return Err(RecorderError::AlreadyRecording);
            }
            *phase = Phase::Busy(RecorderState::Preparing);
        }
        self.emit_state(RecorderState::Preparing);

        match self.prepare(config.clamp(), name).await {
            Ok(session) => {
                let paths = FinishedPaths {
                    project_dir: session.project_dir.clone(),
                    recording_dir: session.recording_dir.clone(),
                };
                let countdown = session.config.countdown_secs;
                *self.inner.phase.lock().await = Phase::Active(Box::new(session));
                self.emit_state(RecorderState::Countdown);
                let this = self.clone();
                tokio::spawn(async move { this.run_countdown(countdown).await });
                Ok(paths)
            }
            Err(e) => {
                reset_global_capture_state();
                *self.inner.phase.lock().await = Phase::Idle;
                self.emit_state(RecorderState::Idle);
                Err(e)
            }
        }
    }

    async fn prepare(&self, config: RecordingConfig, name: String) -> Result<Session, RecorderError> {
        // K-R4: dung lượng ổ
        let free = disk::free_bytes(&config.output_root).map_err(RecorderError::internal)?;
        let required = disk::min_free_to_start();
        if free < required {
            return Err(RecorderError::LowDisk {
                free_bytes: free as f64,
                required_bytes: required as f64,
            });
        }

        let target = resolve_target(&config.target)?;
        if let Some(mic) = &config.mic
            && !MicrophoneFeed::list().contains_key(mic)
        {
            return Err(RecorderError::MicNotFound { name: mic.clone() });
        }
        let camera_info = match &config.camera {
            Some(id) => Some(
                cap_camera::list_cameras()
                    .find(|c| c.device_id() == id)
                    .ok_or_else(|| RecorderError::CameraNotFound { id: id.clone() })?,
            ),
            None => None,
        };

        // Gói project: <root>/<Tên>.mochi/recordings/<id>/ (Vòng 2 thêm project.json).
        let project_dir = unique_project_dir(&config.output_root, &name);
        let recording_id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let recording_dir = project_dir.join("recordings").join(&recording_id);
        std::fs::create_dir_all(recording_dir.parent().unwrap()).map_err(RecorderError::internal)?;
        std::fs::create_dir(&recording_dir).map_err(RecorderError::internal)?;

        let result = self
            .build_actor(&config, target, camera_info, &recording_dir)
            .await;
        let (actor, gate, mic_lock, camera_lock, tasks) = match result {
            Ok(v) => v,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&project_dir);
                return Err(e);
            }
        };

        Ok(Session {
            config,
            project_dir,
            recording_dir,
            name,
            actor,
            gate,
            mic_lock,
            _camera_lock: camera_lock,
            state: RecorderState::Countdown,
            clock: Clock::default(),
            segments: 1,
            mic_muted: false,
            tasks,
            activity: Default::default(),
        })
    }

    #[allow(clippy::type_complexity)]
    async fn build_actor(
        &self,
        config: &RecordingConfig,
        target: cap_recording::screen_capture::ScreenCaptureTarget,
        camera_info: Option<cap_camera::CameraInfo>,
        recording_dir: &Path,
    ) -> Result<
        (
            studio_recording::ActorHandle,
            RecordingStartGate,
            Option<Arc<microphone::MicrophoneFeedLock>>,
            Option<Arc<camera::CameraFeedLock>>,
            Vec<tokio::task::JoinHandle<()>>,
        ),
        RecorderError,
    > {
        let mut tasks = Vec::new();
        let mut builder = studio_recording::Actor::builder(recording_dir.to_path_buf(), target)
            .with_system_audio(config.system_audio);

        // Webcam
        let mut camera_lock = None;
        if let Some(info) = camera_info {
            let feed = CameraFeed::spawn(CameraFeed::default());
            feed.ask(camera::SetInput {
                id: camera::DeviceOrModelID::from_info(&info),
                settings: None,
            })
            .await
            .map_err(|e| device_error(DeviceKind::Camera, e))?
            .await
            .map_err(|e| device_error(DeviceKind::Camera, e))?;
            let this = self.clone();
            let _ = feed
                .ask(camera::OnFeedDisconnect(Box::new(move || {
                    this.warn(RecorderWarning::DeviceLost {
                        kind: DeviceKind::Camera,
                    });
                })))
                .await;
            let lock = Arc::new(
                feed.ask(camera::Lock)
                    .await
                    .map_err(|e| device_error(DeviceKind::Camera, e))?,
            );
            builder = builder.with_camera_feed(lock.clone());
            camera_lock = Some(lock);
        }

        // Mic
        let mut mic_lock = None;
        if let Some(mic) = &config.mic {
            let (error_tx, error_rx) = flume::bounded(16);
            let feed = MicrophoneFeed::spawn(MicrophoneFeed::new(error_tx));
            feed.ask(microphone::SetInput {
                label: mic.clone(),
                settings: None,
            })
            .await
            .map_err(|e| device_error(DeviceKind::Microphone, e))?
            .await
            .map_err(|e| device_error(DeviceKind::Microphone, e))?;
            tokio::time::sleep(Duration::from_millis(100)).await;
            let lock = Arc::new(
                feed.ask(microphone::Lock)
                    .await
                    .map_err(|e| device_error(DeviceKind::Microphone, e))?,
            );
            builder = builder.with_mic_feed(lock.clone());
            mic_lock = Some(lock);
            tasks.push(tokio::spawn(self.clone().watch_mic(mic.clone(), error_rx)));
        }

        // K-R3
        set_keyboard_capture_filter(match config.keystrokes {
            KeystrokeMode::Off => KeyboardCaptureFilter::Off,
            KeystrokeMode::ModifiersOnly => KeyboardCaptureFilter::ModifierCombos,
            KeystrokeMode::All => KeyboardCaptureFilter::All,
        });

        // Health event của pipeline (bản vá [MOCHI] trong cap-recording).
        let (health_tx, health_rx) = mpsc::unbounded_channel();
        set_studio_health_sender(Some(health_tx));
        tasks.push(tokio::spawn(self.clone().watch_health(health_rx)));

        let gate = RecordingStartGate::new();
        let builder = RecordingDefaults::default()
            .apply_to_studio_builder(builder, camera_lock.is_some(), Some(config.fps))
            .with_keyboard_capture(config.keystrokes != KeystrokeMode::Off)
            .with_quality(config.quality.into())
            .with_start_gate(gate.clone());

        let actor = builder.build().await.map_err(|e| RecorderError::StartFailed {
            detail: format!("{e:#}"),
        })?;
        Ok((actor, gate, mic_lock, camera_lock, tasks))
    }

    async fn run_countdown(self, secs: u32) {
        for remaining in (1..=secs).rev() {
            if !self.is_state(RecorderState::Countdown).await {
                return;
            }
            self.emit(RecorderEvent::Countdown { remaining });
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let watch_done;
        {
            let mut phase = self.inner.phase.lock().await;
            let Phase::Active(session) = &mut *phase else {
                return;
            };
            if session.state != RecorderState::Countdown {
                return;
            }
            session.gate.arm();
            session.state = RecorderState::Recording;
            session.clock.started = Some(Instant::now());
            watch_done = session.actor.done_fut();
            session.tasks.push(tokio::spawn(self.clone().monitor_loop()));
            session.tasks.push(tokio::spawn(self.clone().active_window_loop()));
        }
        self.emit_state(RecorderState::Recording);
        info!("Recording started");
        let this = self.clone();
        tokio::spawn(async move { this.watch_done(watch_done).await });
    }

    async fn is_state(&self, state: RecorderState) -> bool {
        matches!(&*self.inner.phase.lock().await, Phase::Active(s) if s.state == state)
    }

    pub async fn pause(&self) -> Result<(), RecorderError> {
        {
            let mut phase = self.inner.phase.lock().await;
            let Phase::Active(session) = &mut *phase else {
                return Err(RecorderError::NotRecording);
            };
            if session.state != RecorderState::Recording {
                return Err(RecorderError::NotRecording);
            }
            session.actor.pause().await.map_err(RecorderError::internal)?;
            session.state = RecorderState::Paused;
            session.clock.paused_since = Some(Instant::now());
        }
        self.emit_state(RecorderState::Paused);
        Ok(())
    }

    pub async fn resume(&self) -> Result<(), RecorderError> {
        {
            let mut phase = self.inner.phase.lock().await;
            let Phase::Active(session) = &mut *phase else {
                return Err(RecorderError::NotRecording);
            };
            if session.state != RecorderState::Paused {
                return Err(RecorderError::NotRecording);
            }
            // Vùng quay phải còn hợp lệ (cửa sổ chưa đóng/thu nhỏ, màn hình còn cắm).
            if let CaptureTarget::Window { id } = &session.config.target
                && window_state(id) != WindowState::Normal
            {
                return Err(RecorderError::TargetNotFound);
            }
            resolve_target(&session.config.target)?;
            session.actor.resume().await.map_err(RecorderError::internal)?;
            session.state = RecorderState::Recording;
            if let Some(since) = session.clock.paused_since.take() {
                session.clock.paused_total += since.elapsed();
            }
            session.segments += 1;
        }
        self.emit_state(RecorderState::Recording);
        Ok(())
    }

    /// K-R2: thao tác duy nhất đổi nguồn được phép khi đang quay.
    pub async fn set_mic_muted(&self, muted: bool) -> Result<(), RecorderError> {
        let mut phase = self.inner.phase.lock().await;
        let Phase::Active(session) = &mut *phase else {
            return Err(RecorderError::NotRecording);
        };
        let Some(lock) = &session.mic_lock else {
            return Err(RecorderError::MicNotFound {
                name: String::new(),
            });
        };
        lock.set_recording_muted(muted);
        session.mic_muted = muted;
        Ok(())
    }

    /// Dừng, finalize (remux, dọn `.recovery-*`, đặt tên) và trả về thông tin bản quay.
    pub async fn stop(&self) -> Result<FinishedRecording, RecorderError> {
        let session = self.take_session(RecorderState::Finalizing).await?;
        self.emit_state(RecorderState::Finalizing);

        // Dừng trong lúc đếm ngược: chưa ghi gì → coi như hủy.
        if !session.gate.is_armed() {
            self.discard_session(session).await;
            return Err(RecorderError::NotRecording);
        }

        let result = self.finalize(session).await;
        reset_global_capture_state();
        *self.inner.phase.lock().await = Phase::Idle;
        self.emit_state(RecorderState::Idle);
        match &result {
            Ok(rec) => self.emit(RecorderEvent::Finished {
                recording: rec.clone(),
            }),
            Err(e) => self.emit(RecorderEvent::Failed { error: e.clone() }),
        }
        result
    }

    /// Hủy: dừng pipeline và xóa toàn bộ gói project vừa tạo.
    pub async fn cancel(&self) -> Result<(), RecorderError> {
        let session = self.take_session(RecorderState::Finalizing).await?;
        self.emit_state(RecorderState::Finalizing);
        self.discard_session(session).await;
        Ok(())
    }

    async fn discard_session(&self, session: Box<Session>) {
        for t in &session.tasks {
            t.abort();
        }
        if let Err(e) = session.actor.cancel().await {
            warn!("cancel actor: {e:#}");
        }
        let project_dir = session.project_dir.clone();
        drop(session);
        if let Err(e) = tokio::fs::remove_dir_all(&project_dir).await {
            warn!("Không xóa được {}: {e}", project_dir.display());
        }
        reset_global_capture_state();
        *self.inner.phase.lock().await = Phase::Idle;
        self.emit_state(RecorderState::Idle);
    }

    async fn take_session(&self, next: RecorderState) -> Result<Box<Session>, RecorderError> {
        let mut phase = self.inner.phase.lock().await;
        match std::mem::replace(&mut *phase, Phase::Busy(next)) {
            Phase::Active(session) => Ok(session),
            other => {
                *phase = other;
                Err(RecorderError::NotRecording)
            }
        }
    }

    async fn finalize(&self, mut session: Box<Session>) -> Result<FinishedRecording, RecorderError> {
        for t in session.tasks.drain(..) {
            t.abort();
        }
        if let Some(since) = session.clock.paused_since.take() {
            session.clock.paused_total += since.elapsed();
        }
        let duration = session.clock.elapsed();

        let completed = session
            .actor
            .stop()
            .await
            .map_err(|e| RecorderError::Internal {
                detail: format!("stop: {e:#}"),
            })?;
        let recording_dir = completed.project_path.clone();
        let remux_dir = recording_dir.clone();
        tokio::task::spawn_blocking(move || {
            cap_recording::recovery::RecoveryManager::remux_if_needed(&remux_dir)
        })
        .await
        .map_err(RecorderError::internal)?
        .map_err(|e| RecorderError::Internal {
            detail: format!("finalize: {e}"),
        })?;

        crate::finalize::cleanup_recovery_workspaces(&recording_dir);
        crate::finalize::set_pretty_name(&recording_dir, &session.name);
        let mut activity = std::mem::take(&mut session.activity).finish(duration.as_secs_f64());
        // R13: lựa chọn highlight lúc quay → mặc định cho clip của bản quay này.
        if session.config.cursor_highlight.enabled {
            activity.cursor_highlight = Some(session.config.cursor_highlight.clone().clamped());
        }
        if let Err(e) = activity.write(&recording_dir) {
            warn!("Không ghi được {}: {e}", mochi_project::sidecar::SIDECAR_FILE);
        }

        Ok(FinishedRecording {
            project_dir: session.project_dir.clone(),
            recording_dir,
            name: session.name.clone(),
            duration_ms: duration.as_secs_f64() * 1000.0,
            segments: session.segments,
            too_short: duration < MIN_KEEP_DURATION,
        })
    }

    /// Xóa một bản quay vừa finalize (người dùng chọn "Bỏ" bản quá ngắn).
    /// Chỉ xóa thư mục `*.mochi` nằm trong `output_root`.
    pub async fn discard_finished(&self, output_root: &Path, project_dir: &Path) -> Result<(), RecorderError> {
        crate::finalize::remove_project_dir(output_root, project_dir).map_err(RecorderError::internal)
    }

    // ---------------- theo dõi trong lúc quay ----------------

    async fn monitor_loop(self) {
        let mut displays = crate::finalize::display_snapshot();
        let mut disk_low_warned = false;
        loop {
            tokio::time::sleep(MONITOR_INTERVAL).await;
            let (state, target, dir, root, elapsed, segments, mic_muted) = {
                let phase = self.inner.phase.lock().await;
                let Phase::Active(s) = &*phase else { return };
                (
                    s.state,
                    s.config.target.clone(),
                    s.project_dir.clone(),
                    s.config.output_root.clone(),
                    s.clock.elapsed(),
                    s.segments,
                    s.mic_muted,
                )
            };

            let bytes = crate::finalize::dir_size(&dir);
            let free = disk::free_bytes(&root).unwrap_or(u64::MAX);
            self.emit(RecorderEvent::Stats {
                stats: RecordingStats {
                    elapsed_ms: elapsed.as_secs_f64() * 1000.0,
                    bytes: bytes as f64,
                    free_bytes: free as f64,
                    segments,
                    mic_muted,
                },
            });

            // K-R4 khi đang quay
            if free < disk::min_free_while_recording() {
                self.warn(RecorderWarning::StoppedDiskFull);
                let this = self.clone();
                tokio::spawn(async move {
                    let _ = this.stop().await;
                });
                return;
            } else if free < disk::min_free_to_start() && !disk_low_warned {
                disk_low_warned = true;
                self.warn(RecorderWarning::DiskLow {
                    free_bytes: free as f64,
                });
            }

            if state != RecorderState::Recording {
                continue;
            }

            // Cửa sổ đang quay bị đóng/thu nhỏ → tự tạm dừng.
            if let CaptureTarget::Window { id } = &target {
                match window_state(id) {
                    WindowState::Normal => {}
                    WindowState::Minimized => {
                        if self.pause().await.is_ok() {
                            self.warn(RecorderWarning::WindowMinimized);
                        }
                        continue;
                    }
                    WindowState::Closed => {
                        if self.pause().await.is_ok() {
                            self.warn(RecorderWarning::WindowClosed);
                        }
                        continue;
                    }
                }
            }

            // Màn hình đổi độ phân giải/DPI hoặc bị rút.
            let now = crate::finalize::display_snapshot();
            if now != displays {
                displays = now;
                if resolve_target(&target).is_err() {
                    if self.pause().await.is_ok() {
                        // Màn hình còn nhưng vùng quay không còn nằm trong nó → báo riêng.
                        let display_alive = match &target {
                            CaptureTarget::Area { display_id, .. } => {
                                resolve_target(&CaptureTarget::Display { id: display_id.clone() }).is_ok()
                            }
                            _ => false,
                        };
                        self.warn(if display_alive {
                            RecorderWarning::AreaOutOfBounds
                        } else {
                            RecorderWarning::DisplayLost
                        });
                    }
                } else if self.pause().await.is_ok() && self.resume().await.is_ok() {
                    self.warn(RecorderWarning::DisplayChanged);
                }
            }
        }
    }

    fn warn(&self, warning: RecorderWarning) {
        warn!(?warning, "recorder warning");
        if let RecorderWarning::DeviceLost { kind } | RecorderWarning::DeviceRestored { kind } = &warning {
            let (kind, lost) = (*kind, matches!(warning, RecorderWarning::DeviceLost { .. }));
            let this = self.clone();
            tokio::spawn(async move {
                let mut phase = this.inner.phase.lock().await;
                if let Phase::Active(s) = &mut *phase {
                    let t = s.clock.elapsed().as_secs_f64();
                    s.activity.device(kind, lost, t);
                }
            });
        }
        self.emit(RecorderEvent::Warning { warning });
    }

    /// R03: ghi cửa sổ đang active mỗi khi đổi (chỉ khi đang quay, mốc theo đồng hồ bản quay).
    async fn active_window_loop(self) {
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let current = crate::activity::foreground_window();
            let mut phase = self.inner.phase.lock().await;
            let Phase::Active(s) = &mut *phase else { return };
            if s.state != RecorderState::Recording {
                continue;
            }
            if let Some((app, title)) = current {
                let t = s.clock.elapsed().as_secs_f64();
                s.activity.window(t, app, title);
            }
        }
    }

    async fn watch_health(
        self,
        mut rx: mpsc::UnboundedReceiver<(&'static str, PipelineHealthEvent)>,
    ) {
        while let Some((track, event)) = rx.recv().await {
            let warning = match event {
                PipelineHealthEvent::FrameDropRateHigh { source, rate_pct } if rate_pct > 5.0 => {
                    Some(RecorderWarning::FrameDrops { source, rate_pct })
                }
                PipelineHealthEvent::DeviceLost { .. } => Some(RecorderWarning::DeviceLost {
                    kind: track_kind(track),
                }),
                PipelineHealthEvent::EncoderRebuilt { backend, .. } => {
                    Some(RecorderWarning::EncoderFallback { backend })
                }
                PipelineHealthEvent::AudioGapDetected { gap_ms } => Some(RecorderWarning::AudioGap {
                    gap_ms: gap_ms as f64,
                }),
                PipelineHealthEvent::DiskSpaceExhausted { .. } => {
                    self.warn(RecorderWarning::StoppedDiskFull);
                    let this = self.clone();
                    tokio::spawn(async move {
                        let _ = this.stop().await;
                    });
                    None
                }
                other => {
                    info!(track, ?other, "pipeline health");
                    None
                }
            };
            if let Some(w) = warning {
                self.warn(w);
            }
        }
    }

    async fn watch_mic(self, name: String, errors: flume::Receiver<cpal_error::StreamError>) {
        let mut lost = false;
        loop {
            tokio::select! {
                err = errors.recv_async() => {
                    let Ok(err) = err else { return };
                    warn!("mic stream error: {err}");
                    if !lost {
                        lost = true;
                        self.warn(RecorderWarning::DeviceLost { kind: DeviceKind::Microphone });
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)), if lost => {
                    if MicrophoneFeed::list().contains_key(&name) {
                        lost = false;
                        self.warn(RecorderWarning::DeviceRestored { kind: DeviceKind::Microphone });
                    }
                }
            }
        }
    }

    /// Pipeline tự kết thúc (lỗi) khi vẫn đang quay → tự dừng và lưu phần đã có.
    async fn watch_done(self, done: cap_recording::DoneFut) {
        let result = done.await;
        let still_active = matches!(
            &*self.inner.phase.lock().await,
            Phase::Active(s) if matches!(s.state, RecorderState::Recording | RecorderState::Paused)
        );
        if !still_active {
            return;
        }
        if let Err(e) = result {
            self.warn(RecorderWarning::PipelineFailed {
                detail: e.to_string(),
            });
            let _ = self.stop().await;
        }
    }
}

/// Đường dẫn trả về ngay khi bắt đầu quay.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FinishedPaths {
    pub project_dir: PathBuf,
    pub recording_dir: PathBuf,
}

mod cpal_error {
    pub use cpal::StreamError;
}

fn reset_global_capture_state() {
    set_studio_health_sender(None);
    set_keyboard_capture_filter(KeyboardCaptureFilter::Off);
}

fn track_kind(track: &str) -> DeviceKind {
    match track {
        "microphone" => DeviceKind::Microphone,
        "camera" => DeviceKind::Camera,
        "system_audio" => DeviceKind::SystemAudio,
        _ => DeviceKind::Screen,
    }
}

/// Lỗi thiết bị → PermissionDenied nếu hệ điều hành chặn quyền (E_ACCESSDENIED).
fn device_error(kind: DeviceKind, e: impl std::fmt::Display) -> RecorderError {
    let detail = e.to_string();
    let lower = detail.to_lowercase();
    if lower.contains("denied") || lower.contains("0x80070005") || lower.contains("permission") {
        RecorderError::PermissionDenied { kind }
    } else {
        RecorderError::DeviceFailed { kind, detail }
    }
}

fn unique_project_dir(root: &Path, name: &str) -> PathBuf {
    let base = crate::finalize::sanitize_file_name(name);
    let mut dir = root.join(format!("{base}.mochi"));
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{base} ({n}).mochi"));
        n += 1;
    }
    dir
}

#[derive(Debug, PartialEq, Eq)]
enum WindowState {
    Normal,
    Minimized,
    Closed,
}

#[cfg(windows)]
fn window_state(id: &str) -> WindowState {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{IsIconic, IsWindow, IsWindowVisible},
    };
    let Ok(raw) = id.parse::<u64>() else {
        return WindowState::Closed;
    };
    let hwnd = HWND(raw as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            WindowState::Closed
        } else if IsIconic(hwnd).as_bool() {
            WindowState::Minimized
        } else {
            WindowState::Normal
        }
    }
}

#[cfg(not(windows))]
fn window_state(_id: &str) -> WindowState {
    WindowState::Normal
}
