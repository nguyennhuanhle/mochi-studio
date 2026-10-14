//! E13 / S07 (v1): chạy thử phiên âm Whisper thật. Tải model tiny vào đúng thư mục model của app
//! (`%APPDATA%\studio.mochi.desktop\models`, app đã cài dùng lại được) nếu chưa có, tạo một đoạn nói
//! tiếng Anh bằng giọng đọc của Windows ra file WAV (không phát ra loa), rồi phiên âm qua đúng đường
//! của app (48 kHz stereo → `to_16k_mono` → `transcribe`) và in các dòng phụ đề kèm thời gian.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

const SENTENCES: [&str; 3] = [
    "Welcome to Mochi Studio.",
    "Press F nine to start recording your screen.",
    "When you stop, the video opens in the editor with automatic zoom.",
];

pub async fn run() -> anyhow::Result<()> {
    mochi_captions::cpu_supported().map_err(anyhow::Error::msg)?;
    let appdata = std::env::var("APPDATA")?;
    let dir = std::path::Path::new(&appdata).join("studio.mochi.desktop").join("models");
    let m = mochi_captions::model("tiny").unwrap();
    if mochi_captions::is_installed(&dir, &m) {
        println!("  model tiny đã có: {}", mochi_captions::model_path(&dir, &m).display());
    } else {
        println!("  tải model tiny ({} byte) vào {} …", m.bytes, dir.display());
        let t = Instant::now();
        let mut last = 0u64;
        let path = mochi_captions::download(&dir, &m, Arc::new(AtomicBool::new(false)), |got, total| {
            let pct = got * 100 / total.max(1);
            if pct >= last + 25 {
                last = pct;
                println!("    {pct}%");
            }
        })
        .map_err(anyhow::Error::msg)?;
        println!(
            "  tải xong sau {:.1} s: {} ({} byte)",
            t.elapsed().as_secs_f64(),
            path.display(),
            std::fs::metadata(&path)?.len()
        );
    }

    // Giọng đọc Windows → WAV (SetOutputToWaveFile: không phát ra loa). Giữa các câu nghỉ 1,5 s.
    let work = std::env::current_dir()?.join("recordings").join("phase6").join("whisper");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let wav = work.join("speech.wav");
    let prompt: String = SENTENCES
        .iter()
        .map(|s| format!("$p.AppendText('{s}'); $p.AppendBreak([TimeSpan]::FromMilliseconds(1500));"))
        .collect();
    let script = format!(
        "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.SelectVoice('Microsoft Zira Desktop'); $s.SetOutputToWaveFile('{}'); \
         $p = New-Object System.Speech.Synthesis.PromptBuilder; {prompt} $s.Speak($p); $s.Dispose()",
        wav.display()
    );
    let st = Command::new("powershell").args(["-NoProfile", "-Command", &script]).status()?;
    anyhow::ensure!(st.success() && wav.exists(), "không tạo được giọng đọc");

    // Giải mã như app: 48 kHz stereo float.
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&wav)
        .args(["-ac", "2", "-ar", "48000", "-f", "f32le", "-"])
        .output()?;
    let pcm48: Vec<f32> = out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    println!("  đoạn nói: {:.2} s, câu gốc:", pcm48.len() as f64 / 2.0 / 48000.0);
    for s in SENTENCES {
        println!("    \"{s}\"");
    }
    let pcm16 = mochi_captions::to_16k_mono(&pcm48);

    let t = Instant::now();
    let caps = mochi_captions::transcribe(
        &mochi_captions::model_path(&dir, &m),
        &pcm16,
        Some("en"),
        mochi_project::Ticks::ZERO,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    println!("  phiên âm xong sau {:.1} s, {} dòng:", t.elapsed().as_secs_f64(), caps.len());
    for c in &caps {
        println!(
            "    [{:6.2} → {:6.2}] {}  ({} từ có mốc thời gian)",
            c.start.secs(),
            c.end.secs(),
            c.text,
            c.words.len()
        );
    }

    // Luật "không phiên âm khi đang quay": `hold` bật → whisper đứng chờ; tắt → chạy tiếp. Rồi thử hủy.
    let hold = Arc::new(AtomicBool::new(true));
    let cancel = Arc::new(AtomicBool::new(false));
    let job = {
        let (model, pcm, hold, cancel) = (mochi_captions::model_path(&dir, &m), pcm16.clone(), hold.clone(), cancel.clone());
        std::thread::spawn(move || {
            let t = Instant::now();
            let r = mochi_captions::transcribe(&model, &pcm, Some("en"), mochi_project::Ticks::ZERO, cancel, hold, |_| {});
            (t.elapsed().as_secs_f64(), r.map(|c| c.len()))
        })
    };
    std::thread::sleep(std::time::Duration::from_secs(3));
    println!("  đang quay (hold) 3 s: phiên âm {}", if job.is_finished() { "ĐÃ CHẠY XONG (sai)" } else { "đứng chờ" });
    hold.store(false, std::sync::atomic::Ordering::Relaxed);
    let (secs, r) = job.join().unwrap();
    println!("  dừng quay → chạy tiếp, xong sau tổng {secs:.1} s: {r:?}");
    let hold = Arc::new(AtomicBool::new(true));
    let cancel2 = Arc::new(AtomicBool::new(false));
    let job = {
        let (model, pcm, hold, cancel) = (mochi_captions::model_path(&dir, &m), pcm16.clone(), hold.clone(), cancel2.clone());
        std::thread::spawn(move || mochi_captions::transcribe(&model, &pcm, Some("en"), mochi_project::Ticks::ZERO, cancel, hold, |_| {}))
    };
    std::thread::sleep(std::time::Duration::from_millis(500));
    cancel2.store(true, std::sync::atomic::Ordering::Relaxed);
    println!("  hủy giữa chừng: {:?}", job.join().unwrap().map(|c| c.len()));

    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}
