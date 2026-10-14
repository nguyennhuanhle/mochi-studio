//! Quản lý cache (`Tên.mochi/cache/{proxies,thumbs,waveforms}`).
//! K-S3: chỉ xóa được thứ nằm TRONG thư mục `cache/` của một gói project — bản quay gốc, media
//! và project.json không bao giờ bị đụng tới.

use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

pub const DEFAULT_LIMIT_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const KINDS: [&str; 3] = ["proxies", "thumbs", "waveforms"];

#[derive(Debug, Clone, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum CacheError {
    /// K-S3: đường dẫn không nằm trong `cache/` của gói project.
    #[error("từ chối xóa ngoài thư mục cache: {}", path.display())]
    #[serde(rename_all = "camelCase")]
    OutsideCache { path: PathBuf },
    #[error("{message}")]
    #[serde(rename_all = "camelCase")]
    Io { message: String },
}

#[derive(Debug, Clone, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    #[specta(type = f64)]
    pub total_bytes: u64,
    #[specta(type = f64)]
    pub proxies_bytes: u64,
    #[specta(type = f64)]
    pub thumbs_bytes: u64,
    #[specta(type = f64)]
    pub waveforms_bytes: u64,
    pub projects: u32,
}

/// Một file trong cache (để dọn theo LRU).
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub project_dir: PathBuf,
    pub kind: &'static str,
    pub path: PathBuf,
    pub bytes: u64,
    pub used: SystemTime,
}

pub fn cache_root(project_dir: &Path) -> PathBuf {
    project_dir.join("cache")
}

pub fn entries(project_dirs: &[PathBuf]) -> Vec<CacheEntry> {
    let mut out = Vec::new();
    for p in project_dirs {
        for kind in KINDS {
            for e in std::fs::read_dir(cache_root(p).join(kind)).into_iter().flatten().flatten() {
                let Ok(meta) = e.metadata() else { continue };
                let bytes = if meta.is_file() {
                    meta.len()
                } else {
                    // Proxy của bản quay là thư mục: tính phần nó thật sự chiếm (ghi lúc tạo proxy).
                    match std::fs::read_to_string(e.path().join(crate::proxy::PROXY_BYTES_FILE)) {
                        Ok(s) => s.trim().parse().unwrap_or(0),
                        Err(_) => continue, // thư mục dở dang / lạ: không đụng tới
                    }
                };
                let used = meta.accessed().or_else(|_| meta.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                out.push(CacheEntry { project_dir: p.clone(), kind, path: e.path(), bytes, used });
            }
        }
    }
    out
}

pub fn usage(project_dirs: &[PathBuf]) -> CacheUsage {
    let mut u = CacheUsage { projects: project_dirs.iter().filter(|p| cache_root(p).is_dir()).count() as u32, ..Default::default() };
    for e in entries(project_dirs) {
        u.total_bytes += e.bytes;
        match e.kind {
            "proxies" => u.proxies_bytes += e.bytes,
            "thumbs" => u.thumbs_bytes += e.bytes,
            _ => u.waveforms_bytes += e.bytes,
        }
    }
    u
}

/// K-S3: xóa `path` CHỈ KHI nó nằm trong `cache/` của gói `project_dir` (so sau khi canonicalize,
/// nên `..`, symlink, đường dẫn tuyệt đối khác đều bị chặn).
pub fn safe_remove(project_dir: &Path, path: &Path) -> Result<(), CacheError> {
    let root = cache_root(project_dir)
        .canonicalize()
        .map_err(|_| CacheError::OutsideCache { path: path.to_path_buf() })?;
    let target = path.canonicalize().map_err(|_| CacheError::OutsideCache { path: path.to_path_buf() })?;
    if target == root || !target.starts_with(&root) {
        return Err(CacheError::OutsideCache { path: path.to_path_buf() });
    }
    let r = if target.is_dir() { std::fs::remove_dir_all(&target) } else { std::fs::remove_file(&target) };
    r.map_err(|e| CacheError::Io { message: e.to_string() })
}

/// Dọn cache của các project (mọi loại, hoặc chỉ `kinds`). Trả về số byte đã giải phóng.
pub fn clear(project_dirs: &[PathBuf], kinds: &[&str]) -> u64 {
    let mut freed = 0;
    for e in entries(project_dirs) {
        if (kinds.is_empty() || kinds.contains(&e.kind)) && safe_remove(&e.project_dir, &e.path).is_ok() {
            freed += e.bytes;
        }
    }
    freed
}

/// Giữ tổng cache dưới `limit`: xóa proxy dùng lâu nhất trước, rồi tới ảnh thu nhỏ / waveform.
/// Trả về các file đã xóa.
pub fn enforce_limit(project_dirs: &[PathBuf], limit: u64) -> Vec<CacheEntry> {
    let mut all = entries(project_dirs);
    let mut total: u64 = all.iter().map(|e| e.bytes).sum();
    if total <= limit {
        return vec![];
    }
    all.sort_by_key(|e| (e.kind != "proxies", e.used));
    let mut removed = Vec::new();
    for e in all {
        if total <= limit {
            break;
        }
        if safe_remove(&e.project_dir, &e.path).is_ok() {
            total -= e.bytes;
            removed.push(e);
        }
    }
    removed
}
