//! Phase 6 lượt D (E11): khử ồn, chuẩn hóa −16 LUFS, nén, ducking — đo bằng đúng bộ trộn của
//! preview / xuất (`OfflineMixer`) trên âm thanh tổng hợp: "giọng" = các đợt âm 220 Hz + họa âm,
//! đợt to xen đợt nhỏ, lẫn tiếng ồn trắng; "nhạc nền" = hợp âm đều.

use cap_audio::AudioData;
use mochi_editor::EditorSession;
use mochi_preview::{OfflineMixer, audiofx};
use mochi_project::{AudioFx, Ticks};
use mochi_timeline::EditOp;
use std::{collections::HashMap, path::PathBuf, process::Command, sync::Arc};

fn ffmpeg(args: &[&str]) -> anyhow::Result<()> {
    let ok = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).status()?.success();
    anyhow::ensure!(ok, "ffmpeg {args:?} thất bại");
    Ok(())
}

fn e(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

fn rms_db(buf: &[f32]) -> f32 {
    let ms = buf.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / buf.len().max(1) as f64;
    if ms <= 1e-12 { -120.0 } else { (10.0 * ms.log10()) as f32 }
}

pub async fn run() -> anyhow::Result<()> {
    let root = std::env::current_dir()?.join("recordings").join("phase6").join("audio");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let voice = root.join("giong.wav");
    let music = root.join("nhac.wav");
    // 10 s: đợt "giọng" 0,6 s mỗi giây (giây chẵn to, giây lẻ nhỏ −20 dB), ồn trắng −38 dBFS suốt.
    ffmpeg(&[
        "-f", "lavfi", "-i",
        "aevalsrc='if(lt(mod(t,1),0.6),(0.5*sin(2*PI*220*t)+0.25*sin(2*PI*440*t)+0.12*sin(2*PI*660*t))*if(mod(floor(t),2),0.1,1),0)':s=48000:d=10",
        "-f", "lavfi", "-i", "anoisesrc=color=white:amplitude=0.012:s=48000:d=10",
        "-filter_complex", "[0][1]amix=inputs=2:normalize=0", "-ac", "2", voice.to_str().unwrap(),
    ])?;
    ffmpeg(&["-f", "lavfi", "-i", "aevalsrc='0.15*sin(2*PI*261.6*t)+0.12*sin(2*PI*329.6*t)+0.1*sin(2*PI*392*t)':s=48000:d=10", "-ac", "2", music.to_str().unwrap()])?;

    let dir = root.join("Am.mochi");
    let mut s = EditorSession::create(&dir, "Am").map_err(e)?;
    let r = s.import(&[voice.clone(), music.clone()]);
    anyhow::ensure!(r.imported.len() == 2, "import lỗi");
    let a1 = s.project().tracks.iter().find(|t| t.kind == mochi_project::TrackKind::Audio).unwrap().id.clone();
    s.apply(&EditOp::AddTrack { kind: mochi_project::TrackKind::Audio }).map_err(e)?;
    let a2 = s.project().tracks.iter().filter(|t| t.kind == mochi_project::TrackKind::Audio).nth(1).unwrap().id.clone();
    let media: Vec<String> = s.project().media.iter().map(|m| m.id.clone()).collect();
    s.apply(&EditOp::AddClip { media_id: media[0].clone(), track_id: Some(a1.clone()), start: Ticks::ZERO }).map_err(e)?;
    let voice_clip = s.project().clips[0].id.clone();
    let paths: HashMap<String, PathBuf> = s.project().media.iter().filter_map(|m| s.media_path(&m.id).map(|p| (m.id.clone(), p))).collect();

    let render = |p: &mochi_project::Project, from: f64, to: f64| -> Vec<f32> {
        let mixer = OfflineMixer::new(Arc::new(p.clone()), &paths);
        let n = ((to - from) * OfflineMixer::RATE as f64) as usize;
        let mut buf = vec![0f32; n * 2];
        mixer.fill(Ticks::from_secs(from), &mut buf);
        buf
    };
    // Khoảng lặng giữa hai đợt (0,7–0,95 s) chỉ có ồn; đợt to 2,1–2,5 s; đợt nhỏ 3,1–3,5 s.
    let set_fx = |s: &mut EditorSession, fx: AudioFx| s.apply(&EditOp::SetAudioFx { clip_id: voice_clip.clone(), fx }).map_err(e);

    println!("== Khử ồn (RNNoise) ==");
    let gap = rms_db(&render(s.project(), 0.7, 0.95));
    set_fx(&mut s, AudioFx { denoise: true, ..Default::default() })?;
    let gap_dn = rms_db(&render(s.project(), 0.7, 0.95));
    let loud_dn = rms_db(&render(s.project(), 2.1, 2.5));
    set_fx(&mut s, AudioFx::default())?;
    let loud = rms_db(&render(s.project(), 2.1, 2.5));
    println!("  ồn trong khoảng lặng: {gap:.1} dB → {gap_dn:.1} dB; đợt giọng to: {loud:.1} dB → {loud_dn:.1} dB");

    println!("== Chuẩn hóa −16 LUFS ==");
    let before = audiofx::analyze(&AudioData::from_samples(render(s.project(), 0.0, 10.0), 2)).integrated;
    set_fx(&mut s, AudioFx { normalize: true, ..Default::default() })?;
    let after = audiofx::analyze(&AudioData::from_samples(render(s.project(), 0.0, 10.0), 2)).integrated;
    println!("  độ to tích hợp: {before:.1} LUFS → {after:.1} LUFS (mục tiêu −16)");

    println!("== Nén ==");
    set_fx(&mut s, AudioFx::default())?;
    let gap_loud_quiet = rms_db(&render(s.project(), 2.1, 2.5)) - rms_db(&render(s.project(), 3.1, 3.5));
    set_fx(&mut s, AudioFx { normalize: true, compress: true, ..Default::default() })?;
    let gap_c = rms_db(&render(s.project(), 2.1, 2.5)) - rms_db(&render(s.project(), 3.1, 3.5));
    println!("  chênh đợt to − đợt nhỏ: {gap_loud_quiet:.1} dB → {gap_c:.1} dB (nén làm đều hơn)");

    println!("== Ducking nhạc nền ==");
    set_fx(&mut s, AudioFx::default())?;
    s.apply(&EditOp::AddClip { media_id: media[1].clone(), track_id: Some(a2.clone()), start: Ticks::ZERO }).map_err(e)?;
    // Đo riêng nhạc: tắt tiếng track giọng nhưng giữ để phát hiện tiếng nói → thay bằng so trực tiếp mức nhạc:
    // tắt giọng bằng âm lượng −60 dB? (ducking dùng độ to đã phân tích của media, không phụ thuộc âm lượng clip).
    s.apply(&EditOp::SetClipAudio { clip_id: voice_clip.clone(), volume_db: Some(-60.0), fade_in: None, fade_out: None, muted: None }).map_err(e)?;
    // Giọng chỉ còn 5 s đầu → từ ~5,5 s là đoạn chỉ có nhạc.
    s.apply(&EditOp::TrimClip { clip_id: voice_clip.clone(), edge: mochi_timeline::Edge::End, to: Ticks::from_secs(5.0) }).map_err(e)?;
    let during_voice = rms_db(&render(s.project(), 2.15, 2.45));
    let between = rms_db(&render(s.project(), 8.0, 8.5));
    s.apply(&EditOp::SetTrack { track_id: a2.clone(), name: None, locked: None, muted: None, hidden: None, volume_db: None, duck: Some(true), magnetic: None }).map_err(e)?;
    let during_voice_d = rms_db(&render(s.project(), 2.15, 2.45));
    let quiet_after = rms_db(&render(s.project(), 8.0, 8.5));
    s.save(false).map_err(e)?;
    println!("  nhạc khi có giọng: {during_voice:.1} dB → {during_voice_d:.1} dB (ducking); đoạn chỉ có nhạc (8 s): {between:.1} dB → {quiet_after:.1} dB (không bị hạ)");
    Ok(())
}
