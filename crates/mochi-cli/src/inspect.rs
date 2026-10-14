//! In thông tin thật của một thư mục bản quay để người xem tự đánh giá.

use std::{path::Path, process::Command};

pub fn run(path: &Path) -> anyhow::Result<()> {
    let meta_path = path.join("recording-meta.json");
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path).map_err(
        |e| anyhow::anyhow!("Không đọc được {}: {e}", meta_path.display()),
    )?)?;

    println!("\n== recording-meta.json ==");
    if let Some(status) = meta.pointer("/status") {
        println!("  status: {status}");
    }
    let segments = meta
        .pointer("/segments")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    println!("  segments: {}", segments.len());

    println!("\n== Files ==");
    let mut files = Vec::new();
    collect_files(path, &mut files)?;
    files.sort();
    for f in &files {
        let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        println!(
            "  {:>12}  {}",
            human_bytes(size),
            f.strip_prefix(path).unwrap_or(f).display()
        );
    }

    println!("\n== Streams (ffprobe) ==");
    for f in files.iter().filter(|f| {
        matches!(
            f.extension().and_then(|e| e.to_str()),
            Some("mp4" | "ogg" | "m4a")
        )
    }) {
        print!("  {}: ", f.strip_prefix(path).unwrap_or(f).display());
        println!("{}", probe(f));
    }

    println!("\n== Con trỏ ==");
    for f in files
        .iter()
        .filter(|f| f.file_name().and_then(|n| n.to_str()) == Some("cursor.json"))
    {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(f)?)?;
        let moves = v.pointer("/moves").and_then(|m| m.as_array()).map_or(0, Vec::len);
        let clicks = v.pointer("/clicks").and_then(|m| m.as_array()).map_or(0, Vec::len);
        println!(
            "  {}: {moves} lần di chuyển, {clicks} sự kiện click",
            f.strip_prefix(path).unwrap_or(f).display()
        );
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            collect_files(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn probe(file: &Path) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate,nb_frames,sample_rate,channels:format=duration",
            "-of",
            "compact=p=0:nk=0",
        ])
        .arg(file)
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" | "),
        Err(e) => format!("(không chạy được ffprobe: {e})"),
    }
}

fn human_bytes(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", units[i])
}
