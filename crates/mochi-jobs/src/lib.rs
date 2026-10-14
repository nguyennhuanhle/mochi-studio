//! Hệ thống (tác vụ nền): S03 proxy, K-S3 dọn cache an toàn, K-S4 hoãn việc nặng khi đang quay.

pub mod cache;
pub mod proxy;
pub mod scheduler;

pub use cache::{CacheError, CacheUsage};
pub use proxy::needs_proxy;
pub use scheduler::{JobScheduler, JobStatus, ProxyJob};

/// Đường dẫn proxy của một media trong gói project.
pub fn proxy_path(project_dir: &std::path::Path, media_id: &str) -> std::path::PathBuf {
    cache::cache_root(project_dir).join("proxies").join(format!("{media_id}.mp4"))
}

/// Proxy của một bản quay là một thư mục (layout Cap) thay vì một file.
pub fn proxy_path_for(project_dir: &std::path::Path, item: &mochi_project::MediaItem) -> std::path::PathBuf {
    match item.kind {
        mochi_project::MediaKind::Recording => cache::cache_root(project_dir).join("proxies").join(format!("{}.rec", item.id)),
        _ => proxy_path(project_dir, &item.id),
    }
}
