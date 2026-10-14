//! Việc sau khi dừng quay và các tiện ích hệ thống tệp.

use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Xóa các thư mục `.recovery-<uuid>/` mà Cap để lại sau khi finalize thành công
/// (docs/round-0.md phát hiện #3). Chỉ xóa khi không còn `.recovery-publication.json`,
/// tức là việc thay segment đã hoàn tất và không cần rollback.
pub fn cleanup_recovery_workspaces(recording_dir: &Path) {
    if recording_dir.join(".recovery-publication.json").exists() {
        warn!("Còn .recovery-publication.json — giữ lại workspace phục hồi");
        return;
    }
    let Ok(entries) = std::fs::read_dir(recording_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_workspace = path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(".recovery-"));
        if is_workspace {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => info!("Đã dọn {}", path.display()),
                Err(e) => warn!("Không dọn được {}: {e}", path.display()),
            }
        }
    }
}

/// Đặt tên hiển thị của bản quay (Cap luôn ghi "Cap <ngày>").
pub fn set_pretty_name(recording_dir: &Path, name: &str) {
    match cap_project::RecordingMeta::load_for_project(recording_dir) {
        Ok(mut meta) => {
            meta.pretty_name = name.to_string();
            if let Err(e) = meta.save_for_project() {
                warn!("Không lưu được tên bản quay: {e:?}");
            }
        }
        Err(e) => warn!("Không đọc được recording-meta.json: {e}"),
    }
}

/// Xóa một gói `*.mochi` — chỉ khi nó nằm ngay trong `output_root`.
pub fn remove_project_dir(output_root: &Path, project_dir: &Path) -> std::io::Result<()> {
    let root = output_root.canonicalize()?;
    let dir = project_dir.canonicalize()?;
    let is_child = dir.parent() == Some(root.as_path());
    let is_project = dir.extension().and_then(|e| e.to_str()) == Some("mochi");
    if !is_child || !is_project {
        return Err(std::io::Error::other(format!(
            "từ chối xóa {} (không phải gói .mochi trong thư mục lưu)",
            dir.display()
        )));
    }
    // Thread decoder có thể đóng file chậm một nhịp sau khi trình phát thả nguồn → thử lại ngắn.
    let mut last = None;
    for _ in 0..15 {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(last.unwrap())
}

pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// (id, rộng, cao vật lý, rộng, cao logic) của mọi màn hình — đổi là biết có cắm / rút / đổi độ
/// phân giải, và đổi DPI (độ phân giải giữ nguyên nhưng cỡ logic đổi).
pub fn display_snapshot() -> Vec<(String, u64, u64, u64, u64)> {
    let mut v: Vec<_> = scap_targets::Display::list()
        .into_iter()
        .map(|d| {
            let size = d.physical_size();
            let logical = d.logical_size();
            (
                d.id().to_string(),
                size.map_or(0, |s| s.width() as u64),
                size.map_or(0, |s| s.height() as u64),
                logical.map_or(0, |s| s.width().round() as u64),
                logical.map_or(0, |s| s.height().round() as u64),
            )
        })
        .collect();
    v.sort();
    v
}

/// Tên file hợp lệ trên Windows: bỏ ký tự cấm, tên dành riêng, khoảng trắng/dấu chấm cuối.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3",
    ];
    if trimmed.is_empty() || reserved.contains(&trimmed.to_uppercase().as_str()) {
        "Mochi recording".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

pub fn project_dir_of(recording_dir: &Path) -> Option<PathBuf> {
    // <project>.mochi/recordings/<id>
    recording_dir.parent()?.parent().map(Path::to_path_buf)
}
