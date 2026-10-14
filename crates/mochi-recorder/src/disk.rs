//! K-R4: không bắt đầu quay khi ổ đích còn < 2 GB; khi đang quay < 500 MB thì dừng an toàn.

use std::path::Path;

pub const MIN_FREE_TO_START: u64 = 2 * 1024 * 1024 * 1024;
pub const MIN_FREE_WHILE_RECORDING: u64 = 500 * 1024 * 1024;

/// Ngưỡng có thể ghi đè bằng biến môi trường để kiểm thử thật (không dùng cho người dùng).
pub fn min_free_to_start() -> u64 {
    mochi_project::test_env("MOCHI_TEST_MIN_FREE_BYTES")
        .and_then(|v| v.parse().ok())
        .unwrap_or(MIN_FREE_TO_START)
}

pub fn min_free_while_recording() -> u64 {
    mochi_project::test_env("MOCHI_TEST_MIN_FREE_WHILE_RECORDING_BYTES")
        .and_then(|v| v.parse().ok())
        .unwrap_or(MIN_FREE_WHILE_RECORDING)
}

/// Dung lượng trống (byte) của ổ chứa `path` (thư mục gần nhất đang tồn tại).
pub fn free_bytes(path: &Path) -> std::io::Result<u64> {
    let mut dir = path;
    while !dir.exists() {
        dir = dir.parent().ok_or_else(|| std::io::Error::other("không có thư mục cha"))?;
    }
    imp::free_bytes(dir)
}

#[cfg(windows)]
mod imp {
    use std::{os::windows::ffi::OsStrExt, path::Path};
    use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::PCWSTR};

    pub fn free_bytes(dir: &Path) -> std::io::Result<u64> {
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free = 0u64;
        unsafe {
            GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        Ok(free)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn free_bytes(_dir: &std::path::Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }
}
