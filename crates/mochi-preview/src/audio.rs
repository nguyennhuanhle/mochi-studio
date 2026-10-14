//! Bộ trộn âm thanh của preview + đồng hồ phát.
//!
//! - Nguồn tiếng (file âm thanh, tiếng của video, mic/âm thanh hệ thống của từng segment
//!   bản quay) được giải mã nền bằng `cap_audio::AudioData` (f32 48 kHz).
//! - Callback cpal trộn mọi clip đang hoạt động theo âm lượng (dB), fade, mute, tốc độ.
//! - Số mẫu đã phát = đồng hồ chuẩn: video bám theo âm thanh, không bao giờ lệch nhau.

use cap_audio::AudioData;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use mochi_project::{ClipContent, MediaKind, Project, Ticks, TrackKind};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

const RATE: f64 = AudioData::SAMPLE_RATE as f64;

/// Một luồng tiếng của media: dữ liệu + độ lệch (giây) so với thời gian của hình.
#[derive(Clone)]
struct Stream {
    data: Arc<AudioData>,
    /// Thời điểm trong media (giây) ứng với mẫu 0 của luồng.
    media_offset: f64,
    /// Chỉ áp dụng trong [from, to) giây của media (segment bản quay).
    from: f64,
    to: f64,
    /// S05: hệ số bù trôi (số giây tiếng ứng với 1 giây hình). 1 = không bù.
    rate: f64,
}

#[derive(Default)]
struct MixState {
    project: Option<Arc<Project>>,
    /// media id → các luồng tiếng đã giải mã.
    streams: HashMap<String, Vec<Stream>>,
    /// E06 (v1): bản quay → thời điểm click (giây trong media), cho tiếng click.
    clicks: HashMap<String, Arc<Vec<f64>>>,
}

pub struct AudioEngine {
    state: Arc<Mutex<MixState>>,
    playing: Arc<AtomicBool>,
    /// Vị trí phát hiện tại, đơn vị tick timeline.
    position: Arc<AtomicI64>,
    _stream: Option<cpal::Stream>,
    has_device: bool,
    loading: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl AudioEngine {
    /// Không mở thiết bị phát (kiểm thử / máy không có loa) — trình phát dùng đồng hồ thật.
    pub fn silent() -> Self {
        Self {
            has_device: false,
            state: Arc::new(Mutex::new(MixState::default())),
            playing: Arc::new(AtomicBool::new(false)),
            position: Arc::new(AtomicI64::new(0)),
            _stream: None,
            loading: Arc::new(Mutex::new(Default::default())),
        }
    }

    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(MixState::default()));
        let playing = Arc::new(AtomicBool::new(false));
        let position = Arc::new(AtomicI64::new(0));
        let stream = open_output(state.clone(), playing.clone(), position.clone())
            .map_err(|e| tracing::warn!("không mở được thiết bị âm thanh: {e:#}"))
            .ok();
        Self {
            has_device: stream.is_some(),
            state,
            playing,
            position,
            _stream: stream,
            loading: Arc::new(Mutex::new(Default::default())),
        }
    }

    /// Có thiết bị phát → âm thanh là đồng hồ chuẩn; không có → trình phát dùng đồng hồ thật.
    pub fn has_device(&self) -> bool {
        self.has_device
    }

    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::SeqCst);
    }

    pub fn seek(&self, t: Ticks) {
        self.position.store(t.0, Ordering::SeqCst);
    }

    pub fn position(&self) -> Ticks {
        Ticks(self.position.load(Ordering::SeqCst))
    }

    /// Bỏ project và mọi luồng tiếng đã giải mã.
    pub fn clear(&self) {
        let mut st = self.state.lock().unwrap();
        st.streams.clear();
        st.project = None;
    }

    /// Cập nhật project; giải mã nền các luồng tiếng còn thiếu.
    pub fn set_project(&self, project: Arc<Project>, paths: &HashMap<String, PathBuf>) {
        let needed: Vec<(String, MediaKind, PathBuf)> = project
            .media
            .iter()
            .filter(|m| m.kind != MediaKind::Image)
            .filter_map(|m| paths.get(&m.id).map(|p| (m.id.clone(), m.kind, p.clone())))
            .collect();
        {
            let mut st = self.state.lock().unwrap();
            st.streams.retain(|id, _| project.media.iter().any(|m| &m.id == id));
            st.project = Some(project);
        }
        for (id, kind, path) in needed {
            let already = self.state.lock().unwrap().streams.contains_key(&id);
            if already || !self.loading.lock().unwrap().insert(id.clone()) {
                continue;
            }
            let state = self.state.clone();
            let loading = self.loading.clone();
            std::thread::Builder::new()
                .name("mochi-audio-decode".into())
                .spawn(move || {
                    let streams = decode_streams(kind, &path);
                    let clicks = (kind == MediaKind::Recording).then(|| Arc::new(mochi_project::autozoom::click_times(&path)));
                    let mut st = state.lock().unwrap();
                    st.streams.insert(id.clone(), streams);
                    if let Some(c) = clicks {
                        st.clicks.insert(id.clone(), c);
                    }
                    drop(st);
                    loading.lock().unwrap().remove(&id);
                })
                .ok();
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn decode_streams(kind: MediaKind, path: &std::path::Path) -> Vec<Stream> {
    let whole = |p: &std::path::Path| -> Option<Stream> {
        AudioData::from_file(p)
            .map_err(|e| tracing::debug!("không có tiếng ở {}: {e}", p.display()))
            .ok()
            .map(|d| Stream {
                data: Arc::new(d),
                media_offset: 0.0,
                from: 0.0,
                to: f64::INFINITY,
                rate: 1.0,
            })
    };
    match kind {
        MediaKind::Audio | MediaKind::Video => whole(path).into_iter().collect(),
        MediaKind::Recording => recording_streams(path),
        MediaKind::Image => vec![],
    }
}

/// Mic + âm thanh hệ thống của từng segment, căn theo `start_time` so với hình (giống Cap).
fn recording_streams(dir: &std::path::Path) -> Vec<Stream> {
    let Ok(meta) = cap_project::RecordingMeta::load_for_project(dir) else {
        return vec![];
    };
    let Some(studio) = meta.studio_meta() else {
        return vec![];
    };
    let mut out = Vec::new();
    let mut media_cursor = 0.0;
    let segments: Vec<_> = match studio {
        cap_project::StudioRecordingMeta::SingleSegment { segment } => vec![(
            segment.display.clone(),
            segment.audio.clone(),
            None,
        )],
        cap_project::StudioRecordingMeta::MultipleSegments { inner } => inner
            .segments
            .iter()
            .map(|s| (s.display.clone(), s.mic.clone(), s.system_audio.clone()))
            .collect(),
    };
    for (display, mic, sys) in segments {
        let display_path = meta.path(&display.path);
        let seg_len = mochi_project::media::probe(&display_path)
            .map(|m| m.duration.secs())
            .unwrap_or(0.0);
        let display_start = display.start_time.unwrap_or(0.0);
        for audio in [mic, sys].into_iter().flatten() {
            let p = meta.path(&audio.path);
            if let Ok(d) = AudioData::from_file(&p) {
                // Mẫu 0 của luồng tiếng ứng với (audio.start - display.start) giây của segment.
                let shift = audio.start_time.unwrap_or(0.0) - display_start;
                let rate = drift_rate(d.sample_count() as f64 / RATE, seg_len - shift);
                if rate != 1.0 {
                    tracing::info!("S05: bù trôi tiếng {} ×{rate:.5}", p.display());
                }
                out.push(Stream {
                    data: Arc::new(d),
                    media_offset: media_cursor + shift,
                    from: media_cursor,
                    to: media_cursor + seg_len,
                    rate,
                });
            }
        }
        media_cursor += seg_len;
    }
    out
}

/// S05: đồng hồ thu tiếng và đồng hồ quay hình lệch nhau rất ít (vài phần nghìn) → kéo giãn tiếng
/// cho khớp đúng độ dài hình. Lệch nhiều hơn 1% là do thiết bị dừng sớm / bắt đầu muộn, không phải
/// trôi đồng hồ → giữ nguyên.
pub fn drift_rate(audio_secs: f64, video_secs: f64) -> f64 {
    if video_secs <= 1.0 {
        return 1.0;
    }
    let diff = audio_secs - video_secs;
    // < 80 ms: đuôi khung AAC cuối + độ trễ lúc dừng, không phải trôi đồng hồ.
    if diff.abs() < 0.08 || (diff / video_secs).abs() > 0.01 {
        return 1.0;
    }
    audio_secs / video_secs
}

/// Bộ trộn dùng khi xuất: cùng hàm `mix` với preview (preview = export), giải mã đồng bộ.
pub struct OfflineMixer {
    st: MixState,
}

impl OfflineMixer {
    pub const RATE: u32 = AudioData::SAMPLE_RATE;
    /// Tick timeline ứng với một mẫu 48 kHz (705.600.000 / 48.000 = 14.700, chẵn).
    pub const TICKS_PER_SAMPLE: i64 = mochi_project::TICKS_PER_SECOND / AudioData::SAMPLE_RATE as i64;

    pub fn new(project: Arc<Project>, paths: &HashMap<String, PathBuf>) -> Self {
        let jobs: Vec<_> = project
            .media
            .iter()
            .filter(|m| m.kind != MediaKind::Image)
            .filter_map(|m| paths.get(&m.id).map(|p| (m.id.clone(), m.kind, p.clone())))
            .map(|(id, kind, path)| {
                std::thread::spawn(move || {
                    let clicks = (kind == MediaKind::Recording).then(|| Arc::new(mochi_project::autozoom::click_times(&path)));
                    (id, decode_streams(kind, &path), clicks)
                })
            })
            .collect();
        let mut streams = HashMap::new();
        let mut clicks = HashMap::new();
        for (id, s, c) in jobs.into_iter().filter_map(|j| j.join().ok()) {
            if let Some(c) = c {
                clicks.insert(id.clone(), c);
            }
            streams.insert(id, s);
        }
        Self { st: MixState { project: Some(project), streams, clicks } }
    }

    /// Có clip nào phát ra tiếng không (không thì khỏi tạo luồng AAC).
    pub fn has_audio(&self) -> bool {
        let Some(p) = &self.st.project else { return false };
        p.clips.iter().any(|c| {
            let clicks = matches!(&c.content, ClipContent::Recording { fx } if fx.cursor.click_sound.enabled)
                && c.media_id.as_ref().is_some_and(|id| self.st.clicks.get(id).is_some_and(|k| !k.is_empty()));
            !c.muted
                && (clicks || c.media_id.as_ref().is_some_and(|id| self.st.streams.get(id).is_some_and(|s| !s.is_empty())))
                && p.tracks.iter().any(|t| t.id == c.track_id && !t.muted && t.kind != TrackKind::Overlay)
        })
    }

    /// Trộn stereo xen kẽ, bắt đầu ở tick `start` của timeline; `out.len()` = 2 × số mẫu.
    pub fn fill(&self, start: Ticks, out: &mut [f32]) {
        out.fill(0.0);
        mix(&self.st, start.0, Self::TICKS_PER_SAMPLE as f64, out.len() / 2, 2, out);
    }
}

fn open_output(
    state: Arc<Mutex<MixState>>,
    playing: Arc<AtomicBool>,
    position: Arc<AtomicI64>,
) -> anyhow::Result<cpal::Stream> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("không có loa"))?;
    let config = device.default_output_config()?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0 as f64;
    let stream_config: cpal::StreamConfig = config.into();
    let tick_per_frame = mochi_project::TICKS_PER_SECOND as f64 / rate;
    let stream = device.build_output_stream(
        &stream_config,
        move |out: &mut [f32], _| {
            out.fill(0.0);
            if !playing.load(Ordering::SeqCst) {
                return;
            }
            let frames = out.len() / channels;
            let start = position.load(Ordering::SeqCst);
            if let Ok(st) = state.try_lock() {
                mix(&st, start, tick_per_frame, frames, channels, out);
            }
            position.store(
                start + (frames as f64 * tick_per_frame).round() as i64,
                Ordering::SeqCst,
            );
        },
        |e| tracing::warn!("audio output: {e}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

/// E06 (v1): tiếng "tách" ngắn (~40 ms) tổng hợp, `t` giây tính từ lúc click.
const CLICK_LEN: f64 = 0.04;
fn click_wave(t: f64) -> f32 {
    let env = (-t / 0.0045).exp();
    (env * (0.6 * (std::f64::consts::TAU * 3200.0 * t).sin() + 0.4 * (std::f64::consts::TAU * 1400.0 * t).sin()) * 0.5) as f32
}

/// Cộng tiếng click của clip bản quay (đã bật) vào `out`.
fn mix_clicks(st: &MixState, clip: &mochi_project::Clip, gain: f32, start: i64, tick_per_frame: f64, frames: usize, channels: usize, out: &mut [f32]) {
    let ClipContent::Recording { fx } = &clip.content else { return };
    if !fx.cursor.click_sound.enabled || clip.hold.is_some() {
        return;
    }
    let Some(clicks) = clip.media_id.as_ref().and_then(|id| st.clicks.get(id)) else { return };
    let tps = mochi_project::TICKS_PER_SECOND as f64;
    let vol = gain * (fx.cursor.click_sound.volume / 100.0) as f32;
    let (src_in, src_out) = (clip.source_in.secs(), clip.source_out.secs());
    let win_start = start as f64;
    let win_end = start as f64 + frames as f64 * tick_per_frame;
    for &c in clicks.iter().filter(|&&c| c >= src_in && c < src_out) {
        let at = clip.start.0 as f64 + (c - src_in) / clip.speed * tps;
        if at + CLICK_LEN * tps < win_start || at > win_end {
            continue;
        }
        let i0 = (((at - win_start) / tick_per_frame).floor().max(0.0)) as usize;
        let i1 = ((((at + CLICK_LEN * tps) - win_start) / tick_per_frame).ceil().max(0.0) as usize).min(frames);
        for i in i0..i1 {
            let dt = (win_start + i as f64 * tick_per_frame - at) / tps;
            if (0.0..CLICK_LEN).contains(&dt) {
                let v = click_wave(dt) * vol;
                for ch in 0..channels {
                    out[i * channels + ch] += v;
                }
            }
        }
    }
}

fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Trộn `frames` mẫu bắt đầu ở tick `start` vào `out` (xen kẽ `channels` kênh).
fn mix(st: &MixState, start: i64, tick_per_frame: f64, frames: usize, channels: usize, out: &mut [f32]) {
    let Some(project) = &st.project else { return };
    let end = start + (frames as f64 * tick_per_frame) as i64;
    for clip in &project.clips {
        // Freeze frame (E04) không có tiếng.
        if clip.muted || clip.hold.is_some() || clip.start.0 >= end || clip.end().0 <= start {
            continue;
        }
        let Some(track) = project.tracks.iter().find(|t| t.id == clip.track_id) else {
            continue;
        };
        if track.muted || track.kind == TrackKind::Overlay {
            continue;
        }
        if matches!(clip.content, ClipContent::Image) || clip.content.is_overlay() {
            continue;
        }
        let gain = db_to_gain(clip.volume_db + track.volume_db);
        mix_clicks(st, clip, gain, start, tick_per_frame, frames, channels, out);
        let Some(streams) = clip.media_id.as_ref().and_then(|id| st.streams.get(id)) else {
            continue;
        };
        let dur = clip.duration().0 as f64;
        for i in 0..frames {
            let t = start as f64 + i as f64 * tick_per_frame;
            if t < clip.start.0 as f64 || t >= clip.end().0 as f64 {
                continue;
            }
            let local = t - clip.start.0 as f64;
            let mut env = 1.0f64;
            if clip.fade_in.0 > 0 && local < clip.fade_in.0 as f64 {
                env = local / clip.fade_in.0 as f64;
            }
            if clip.fade_out.0 > 0 && dur - local < clip.fade_out.0 as f64 {
                env = env.min((dur - local) / clip.fade_out.0 as f64);
            }
            let media_secs =
                (clip.source_in.0 as f64 + local * clip.speed) / mochi_project::TICKS_PER_SECOND as f64;
            let g = gain * env as f32;
            for s in streams {
                if media_secs < s.from || media_secs >= s.to {
                    continue;
                }
                let idx = ((media_secs - s.media_offset) * RATE * s.rate) as i64;
                if idx < 0 || idx as usize >= s.data.sample_count() {
                    continue;
                }
                let ch = s.data.channels() as usize;
                let base = idx as usize * ch;
                let samples = s.data.samples();
                for c in 0..channels {
                    let v = samples[base + c.min(ch - 1)];
                    out[i * channels + c] += v * g;
                }
            }
        }
    }
    for v in out.iter_mut() {
        *v = v.clamp(-1.0, 1.0);
    }
}

/// Đỉnh âm lượng (trung bình trị tuyệt đối) mỗi 100 ms của toàn bộ media, để vẽ waveform.
/// Bản quay: cộng mic + âm thanh hệ thống theo đúng vị trí trong media.
pub fn waveform(kind: MediaKind, path: &std::path::Path) -> Vec<f32> {
    const BLOCK: f64 = 0.1;
    let streams = decode_streams(kind, path);
    let end = streams
        .iter()
        .map(|s| (s.media_offset + s.data.sample_count() as f64 / RATE).min(s.to))
        .fold(0.0, f64::max);
    let blocks = (end / BLOCK).ceil().max(0.0) as usize;
    let mut out = vec![0f32; blocks];
    for s in &streams {
        let ch = s.data.channels() as usize;
        let samples = s.data.samples();
        for (b, v) in out.iter_mut().enumerate() {
            let t0 = b as f64 * BLOCK;
            if t0 < s.from || t0 >= s.to {
                continue;
            }
            let i0 = ((t0 - s.media_offset) * RATE).max(0.0) as usize;
            let i1 = (((t0 + BLOCK) - s.media_offset) * RATE).max(0.0) as usize;
            let i1 = i1.min(s.data.sample_count());
            if i0 >= i1 {
                continue;
            }
            let sum: f32 = (i0..i1).map(|i| samples[i * ch].abs()).sum();
            *v += sum / (i1 - i0) as f32;
        }
    }
    out.iter_mut().for_each(|v| *v = v.min(1.0));
    out
}
