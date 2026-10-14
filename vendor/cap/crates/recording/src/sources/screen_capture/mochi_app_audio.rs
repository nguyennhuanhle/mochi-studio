//! [MOCHI] R10 (v1): thu tiếng của riêng một ứng dụng (WASAPI process loopback, Windows 10 2004+).
//!
//! Khi `MOCHI_APP_AUDIO_PID` khác 0, nguồn "âm thanh hệ thống" của Cap lấy tiếng từ cây tiến trình
//! của PID đó thay vì cả thiết bị phát mặc định. Mọi phần còn lại (căn giờ theo QPC, chia đoạn khi
//! tạm dừng, mã hóa, `recording-meta.json`) đi đường cũ của Cap.

use crate::{AudioFrame, output_pipeline};
use anyhow::{Context, anyhow};
use cap_timestamp::{PerformanceCounterTimestamp, Timestamp};
use futures::channel::mpsc;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tracing::{info, warn};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
            AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
            AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
            ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
            IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
            IAudioCaptureClient, IAudioClient, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
        },
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT},
            Threading::{CreateEventW, WaitForSingleObject},
            Variant::VT_BLOB,
        },
    },
    core::{HRESULT, IUnknown, Interface, implement},
};

/// PID của ứng dụng cần thu tiếng; 0 = tắt (dùng âm thanh hệ thống như Cap gốc).
pub static MOCHI_APP_AUDIO_PID: AtomicU32 = AtomicU32::new(0);
/// Bật lên khi không mở được process loopback và đã quay lại âm thanh hệ thống.
pub static MOCHI_APP_AUDIO_FELL_BACK: AtomicBool = AtomicBool::new(false);

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
/// Không có gói nào lâu hơn chừng này (app đang im) → chèn lặng để luồng tiếng liền mạch.
const GAP_FILL_AFTER: Duration = Duration::from_millis(200);
/// Chỉ chèn lặng tới (bây giờ − độ trễ này) để gói thật đến muộn không chồng lên.
const GAP_FILL_LAG: Duration = Duration::from_millis(100);
const SILENCE_CHUNK: usize = 960; // 20 ms

pub fn app_audio_pid() -> Option<u32> {
    match MOCHI_APP_AUDIO_PID.load(Ordering::Relaxed) {
        0 => None,
        pid => Some(pid),
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion(std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _op: windows::core::Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        if let Some(tx) = self.0.lock().ok().and_then(|mut g| g.take()) {
            let _ = tx.send(());
        }
        Ok(())
    }
}

fn format() -> WAVEFORMATEX {
    let block = CHANNELS * 4;
    WAVEFORMATEX {
        wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
        nChannels: CHANNELS,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * u32::from(block),
        nBlockAlign: block,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

/// Mở IAudioClient process loopback cho cây tiến trình `pid` (chạy trên luồng MTA).
unsafe fn activate(pid: u32) -> anyhow::Result<IAudioClient> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    // PROPVARIANT của windows-rs gọi PropVariantClear khi drop → sẽ CoTaskMemFree blob đang nằm trên
    // stack (hỏng heap). Blob là của ta, không được để nó giải phóng.
    let mut pv = std::mem::ManuallyDrop::new(PROPVARIANT::default());
    unsafe {
        let inner = &mut pv.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob.cbSize = std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
        inner.Anonymous.blob.pBlobData = &mut params as *mut _ as *mut u8;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler =
        Completion(std::sync::Mutex::new(Some(tx))).into();
    let op = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*pv),
            &handler,
        )
    }
    .context("ActivateAudioInterfaceAsync")?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| anyhow!("process loopback activation timed out"))?;

    let mut hr = HRESULT(0);
    let mut unk: Option<IUnknown> = None;
    unsafe { op.GetActivateResult(&mut hr, &mut unk) }.context("GetActivateResult")?;
    hr.ok().context("process loopback activation")?;
    let client: IAudioClient = unk.ok_or_else(|| anyhow!("no audio client"))?.cast()?;

    let fmt = format();
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            200_000,
            0,
            &fmt,
            None,
        )
    }
    .context("IAudioClient::Initialize")?;
    Ok(client)
}

fn silence(samples: usize) -> ffmpeg::frame::Audio {
    let info = crate::sources::audio_mixer::AudioMixer::INFO;
    let mut f = info.empty_frame(samples);
    for i in 0..f.planes() {
        f.data_mut(i).fill(0);
    }
    f
}

pub(super) struct ProcessLoopback {
    play: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ProcessLoopback {
    /// Mở luồng thu (chưa phát gói nào cho tới `play`). Lỗi ở đây → người gọi quay về âm thanh hệ thống.
    pub fn open(
        pid: u32,
        tx: mpsc::Sender<AudioFrame>,
        health_tx: output_pipeline::HealthSender,
    ) -> anyhow::Result<Self> {
        let play = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();
        let thread = std::thread::Builder::new()
            .name("mochi-app-audio".into())
            .spawn({
                let (play, stop) = (play.clone(), stop.clone());
                move || unsafe { run(pid, tx, health_tx, play, stop, ready_tx) }
            })?;
        match ready_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(())) => {
                info!(pid, "App audio (process loopback) ready");
                Ok(Self {
                    play,
                    stop,
                    thread: Some(thread),
                })
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err(anyhow!("process loopback thread did not report"))
            }
        }
    }

    pub fn play(&self) {
        self.play.store(true, Ordering::Relaxed);
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ProcessLoopback {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn run(
    pid: u32,
    mut tx: mpsc::Sender<AudioFrame>,
    health_tx: output_pipeline::HealthSender,
    play: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    ready: std::sync::mpsc::Sender<anyhow::Result<()>>,
) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let setup = (|| -> anyhow::Result<(IAudioClient, IAudioCaptureClient, HANDLE)> {
        let client = unsafe { activate(pid) }?;
        let event = unsafe { CreateEventW(None, false, false, None) }?;
        unsafe { client.SetEventHandle(event) }?;
        let capture: IAudioCaptureClient = unsafe { client.GetService() }?;
        Ok((client, capture, event))
    })();
    let (client, capture, event) = match setup {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            if com.is_ok() {
                unsafe { CoUninitialize() };
            }
            return;
        }
    };

    while !play.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(2));
    }
    if !stop.load(Ordering::Relaxed) {
        if let Err(e) = unsafe { client.Start() } {
            warn!("app audio Start: {e}");
        }
    }

    let mut next: Option<PerformanceCounterTimestamp> = None;
    let mut send = |frame: ffmpeg::frame::Audio,
                    ts: PerformanceCounterTimestamp,
                    next: &mut Option<PerformanceCounterTimestamp>| {
        *next = Some(ts + Duration::from_secs_f64(frame.samples() as f64 / f64::from(RATE)));
        let _ = output_pipeline::send_with_stall_budget_futures(
            &mut tx,
            AudioFrame::new(frame, Timestamp::PerformanceCounter(ts)),
            "mochi-app-audio",
            &health_tx,
        );
    };
    let info = crate::sources::audio_mixer::AudioMixer::INFO;

    while !stop.load(Ordering::Relaxed) {
        let woke = unsafe { WaitForSingleObject(event, 50) } == WAIT_OBJECT_0;
        let mut got = false;
        if woke {
            loop {
                let Ok(n) = (unsafe { capture.GetNextPacketSize() }) else {
                    break;
                };
                if n == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags, mut qpc) = (std::ptr::null_mut(), 0u32, 0u32, 0u64);
                if unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc)) }
                    .is_err()
                {
                    break;
                }
                if frames > 0 {
                    let mut ts = PerformanceCounterTimestamp::from_100ns(qpc as i64);
                    // Gói đến sau đoạn lặng đã chèn → nối tiếp, không chồng.
                    if let Some(n) = next {
                        let gap = ts.signed_duration_since_secs(n);
                        if gap < 0.0 {
                            ts = n;
                        } else if gap > 0.005 {
                            // Lỗ giữa đoạn đã gửi và gói này (app vừa phát lại) → lấp lặng.
                            let mut fill = (gap * f64::from(RATE)) as usize;
                            while fill > 0 {
                                let k = fill.min(SILENCE_CHUNK);
                                let at = next.unwrap_or(n);
                                send(silence(k), at, &mut next);
                                fill -= k;
                            }
                            ts = next.unwrap_or(ts);
                        }
                    }
                    let frame = if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                        silence(frames as usize)
                    } else {
                        let bytes = frames as usize * usize::from(CHANNELS) * 4;
                        info.wrap_frame(unsafe { std::slice::from_raw_parts(data, bytes) })
                    };
                    send(frame, ts, &mut next);
                    got = true;
                }
                let _ = unsafe { capture.ReleaseBuffer(frames) };
            }
        }
        // App im lặng: WASAPI không phát gói → chèn lặng để không có lỗ trong luồng tiếng.
        if !got && let Some(n) = next {
            let now = PerformanceCounterTimestamp::now();
            let behind = now.signed_duration_since_secs(n);
            if behind > GAP_FILL_AFTER.as_secs_f64() {
                let mut fill = ((behind - GAP_FILL_LAG.as_secs_f64()) * f64::from(RATE)) as usize;
                while fill > 0 {
                    let k = fill.min(SILENCE_CHUNK);
                    let ts = next.unwrap_or(n);
                    send(silence(k), ts, &mut next);
                    fill -= k;
                }
            }
        } else if !got && next.is_none() && play.load(Ordering::Relaxed) {
            // Chưa có gói nào từ lúc bắt đầu (app chưa phát tiếng): neo luồng ở thời điểm hiện tại.
            let now = PerformanceCounterTimestamp::now();
            send(silence(SILENCE_CHUNK), now - Duration::from_millis(20), &mut next);
        }
    }

    let _ = unsafe { client.Stop() };
    let _ = unsafe { CloseHandle(event) };
    drop(capture);
    drop(client);
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
}
