//! E11 (v1): xử lý tiếng chạy trên máy.
//! - Phân tích độ to theo ITU-R BS.1770 (lọc K, độ to "momentary" 400 ms lấy mẫu 100 lần / giây,
//!   độ to tích hợp có cổng −70 LUFS / −10 LU) → chuẩn hóa về −16 LUFS, nén, ducking.
//! - Khử ồn bằng RNNoise (có sẵn trong Cap), bù độ trễ của bộ lọc để tiếng vẫn khớp hình.

use cap_audio::AudioData;

/// Độ to theo thời gian của một luồng tiếng.
pub struct Analysis {
    /// LUFS "momentary" (cửa sổ 400 ms) mỗi 10 ms; −120 = im lặng.
    pub momentary: Vec<f32>,
    /// LUFS tích hợp (có cổng); −120 = im lặng.
    pub integrated: f32,
}

pub const ENVELOPE_HZ: f64 = 100.0;
const RATE: f64 = AudioData::SAMPLE_RATE as f64;

struct Biquad {
    b: [f64; 3],
    a: [f64; 3],
    z: [f64; 2],
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[1] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[2] * y;
        y
    }
}

/// Lọc K (BS.1770-4) cho 48 kHz: kệ cao + thông cao.
fn k_filter() -> (Biquad, Biquad) {
    (
        Biquad { b: [1.53512485958697, -2.69169618940638, 1.19839281085285], a: [1.0, -1.69065929318241, 0.73248077421585], z: [0.0; 2] },
        Biquad { b: [1.0, -2.0, 1.0], a: [1.0, -1.99004745483398, 0.99007225036621], z: [0.0; 2] },
    )
}

fn lufs(ms: f64) -> f32 {
    if ms <= 1e-12 { -120.0 } else { (-0.691 + 10.0 * ms.log10()) as f32 }
}

pub fn analyze(data: &AudioData) -> Analysis {
    let ch = data.channels().max(1) as usize;
    let s = data.samples();
    let frames = s.len() / ch;
    let block = (RATE / ENVELOPE_HZ) as usize; // 10 ms
    let mut filters: Vec<(Biquad, Biquad)> = (0..ch).map(|_| k_filter()).collect();
    // Năng lượng (đã lọc K) của từng khối 10 ms, cộng các kênh (trọng số 1 cho L/R).
    let mut blocks = Vec::with_capacity(frames / block + 1);
    let mut acc = 0.0f64;
    for i in 0..frames {
        for (c, f) in filters.iter_mut().enumerate() {
            let y = f.1.run(f.0.run(s[i * ch + c] as f64));
            acc += y * y;
        }
        if (i + 1) % block == 0 {
            blocks.push(acc / block as f64);
            acc = 0.0;
        }
    }
    // Momentary: trung bình 40 khối (400 ms) quanh mỗi mốc.
    let mut momentary = Vec::with_capacity(blocks.len());
    let mut window = 0.0f64;
    for i in 0..blocks.len() {
        window += blocks[i];
        if i >= 40 {
            window -= blocks[i - 40];
        }
        momentary.push(lufs(window / (i + 1).min(40) as f64));
    }
    // Tích hợp: khối 400 ms chồng 75 % (bước 100 ms = 10 khối), cổng tuyệt đối −70 rồi tương đối −10.
    let gates: Vec<f64> = (0..blocks.len().saturating_sub(39))
        .step_by(10)
        .map(|i| blocks[i..i + 40].iter().sum::<f64>() / 40.0)
        .filter(|ms| lufs(*ms) > -70.0)
        .collect();
    let integrated = if gates.is_empty() {
        -120.0
    } else {
        let mean = gates.iter().sum::<f64>() / gates.len() as f64;
        let rel = lufs(mean) - 10.0;
        let kept: Vec<f64> = gates.into_iter().filter(|ms| lufs(*ms) > rel).collect();
        if kept.is_empty() { -120.0 } else { lufs(kept.iter().sum::<f64>() / kept.len() as f64) }
    };
    Analysis { momentary, integrated }
}

/// Khử ồn RNNoise từng kênh (khung 480 mẫu ở 48 kHz). Đầu ra lệch đúng `DELAY_SAMPLES` so với
/// đầu vào → bỏ phần trễ đầu và đệm cuối để giữ nguyên thời gian.
pub fn denoise(data: &AudioData) -> AudioData {
    use cap_rnnoise::{DELAY_SAMPLES, DenoiseState, FRAME_SIZE};
    let ch = data.channels().max(1) as usize;
    let s = data.samples();
    let frames = s.len() / ch;
    let mut out = vec![0f32; s.len()];
    for c in 0..ch {
        let mut st = DenoiseState::new();
        let mut processed: Vec<f32> = Vec::with_capacity(frames + DELAY_SAMPLES + FRAME_SIZE);
        let (mut inp, mut o) = ([0f32; FRAME_SIZE], [0f32; FRAME_SIZE]);
        let total = frames + DELAY_SAMPLES;
        let mut i = 0;
        while i < total {
            for k in 0..FRAME_SIZE {
                let idx = i + k;
                inp[k] = if idx < frames { s[idx * ch + c] * 32768.0 } else { 0.0 };
            }
            st.process_frame(&mut o, &inp);
            processed.extend(o.iter().map(|v| v / 32768.0));
            i += FRAME_SIZE;
        }
        for k in 0..frames {
            out[k * ch + c] = processed.get(k + DELAY_SAMPLES).copied().unwrap_or(0.0);
        }
    }
    AudioData::from_samples(out, ch as u16)
}

/// Nén: ngưỡng −24 LUFS, tỉ lệ 3:1, bù +3 dB. Trả về dB cộng thêm cho độ to `level`.
pub fn compressor_db(level: f32) -> f32 {
    let over = level + 24.0;
    let reduction = if over > 0.0 { over * (1.0 - 1.0 / 3.0) } else { 0.0 };
    3.0 - reduction
}

/// Chuẩn hóa về −16 LUFS (kẹp ±20 dB; im lặng → 0).
pub fn normalize_db(integrated: f32) -> f32 {
    if integrated <= -70.0 { 0.0 } else { (-16.0 - integrated).clamp(-20.0, 20.0) }
}

/// Mức ducking (dB) khi có tiếng nói.
pub const DUCK_DB: f32 = -14.0;
/// Ngưỡng coi là "đang nói" (LUFS momentary).
pub const VOICE_LUFS: f32 = -45.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, secs: f64, amp: f32) -> AudioData {
        let n = (RATE * secs) as usize;
        let mut v = Vec::with_capacity(n * 2);
        for i in 0..n {
            let x = (std::f64::consts::TAU * freq * i as f64 / RATE).sin() as f32 * amp;
            v.push(x);
            v.push(x);
        }
        AudioData::from_samples(v, 2)
    }

    #[test]
    fn full_scale_1khz_stereo_is_about_0_lufs() {
        // BS.1770: sin 1 kHz 0 dBFS một kênh = −3,01 LUFS; hai kênh giống nhau cộng năng lượng → ≈ 0 LUFS.
        let a = analyze(&sine(1000.0, 3.0, 1.0));
        assert!((a.integrated - 0.0).abs() < 0.6, "{}", a.integrated);
        let q = analyze(&sine(1000.0, 3.0, 0.1));
        assert!((q.integrated - (-20.0)).abs() < 0.6, "{}", q.integrated);
    }

    #[test]
    fn silence_is_gated() {
        let a = analyze(&AudioData::from_samples(vec![0.0; 48_000 * 2 * 2], 2));
        assert_eq!(a.integrated, -120.0);
        assert_eq!(normalize_db(a.integrated), 0.0);
    }
}
