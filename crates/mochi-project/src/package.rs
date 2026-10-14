//! Gói project `Tên.mochi/` trên đĩa: lock (K-E3), ProjectFs (K-E1), lưu an toàn + backup (S01),
//! kiểm tra phiên bản schema (K-E6).
//!
//! ```text
//! Tên.mochi/
//!   project.json
//!   .lock                      handle mở độc quyền khi project đang mở
//!   recordings/<id>/           bản quay (layout Cap), chỉ đọc
//!   cache/                     proxy, waveform, thumbnail — xóa được bất cứ lúc nào
//!   backups/project.<ts>.json  10 bản gần nhất + pre-migrate-vN.json
//! ```

use crate::{BackupInfo, Project, ProjectError, SCHEMA_VERSION};
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
};

pub const PROJECT_FILE: &str = "project.json";
pub const LOCK_FILE: &str = ".lock";
pub const BACKUP_DIR: &str = "backups";
pub const CACHE_DIR: &str = "cache";
pub const RECORDINGS_DIR: &str = "recordings";
pub const MAX_BACKUPS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    /// Tự lưu nền (thưa: tối đa mỗi 10 phút một bản).
    Auto,
    /// Người dùng lưu (Ctrl+S, đóng project).
    Manual,
}

/// Lần mở trước không đóng sạch (crash) và project không đang mở ở đâu khác (S02).
pub fn is_unclean(dir: &Path) -> bool {
    let Ok(s) = std::fs::read_to_string(dir.join(LOCK_FILE)) else {
        return false;
    };
    if !s.contains("state=open") {
        return false;
    }
    // Đang mở ở process khác → không phải crash.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let probe = std::fs::OpenOptions::new().read(true).share_mode(0).open(dir.join(LOCK_FILE));
        if probe.as_ref().err().is_some_and(is_sharing_violation) {
            return false;
        }
    }
    true
}

/// Một project đang mở. Giữ handle `.lock` mở độc quyền suốt phiên làm việc: hệ điều hành
/// tự nhả khi process thoát (kể cả crash), nên không có "lock mồ côi".
pub struct ProjectPackage {
    dir: PathBuf,
    lock: File,
    /// Lần mở trước không đóng sạch (app crash / mất điện).
    recovered: bool,
}

impl ProjectPackage {
    /// Mở (hoặc tạo) gói và giành khóa. K-E3: đang mở ở process khác → `Locked`.
    pub fn acquire(dir: &Path) -> Result<Self, ProjectError> {
        std::fs::create_dir_all(dir)?;
        // Nội dung .lock của lần trước: còn "state=open" nghĩa là phiên đó không đóng sạch.
        let recovered = std::fs::read_to_string(dir.join(LOCK_FILE))
            .map(|s| s.contains("state=open"))
            .unwrap_or(false);
        let lock = open_exclusive(&dir.join(LOCK_FILE)).map_err(|e| {
            if is_sharing_violation(&e) {
                ProjectError::Locked
            } else {
                ProjectError::from(e)
            }
        })?;
        use std::io::Write;
        let mut l = &lock;
        let _ = writeln!(l, "state=open pid={}", std::process::id());
        Ok(Self {
            dir: dir.to_path_buf(),
            lock,
            recovered,
        })
    }

    /// Lần mở trước bị đóng đột ngột → đang dùng bản tự lưu gần nhất.
    pub fn recovered(&self) -> bool {
        self.recovered
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn exists(dir: &Path) -> bool {
        dir.join(PROJECT_FILE).is_file()
    }

    /// Đọc project.json. Hỏng → `Corrupt` kèm danh sách backup. Mới hơn → `NewerSchema`.
    /// Cũ hơn → sao lưu `pre-migrate-vN.json` rồi nâng cấp.
    pub fn load(&self) -> Result<Project, ProjectError> {
        Self::load_file(&self.dir.join(PROJECT_FILE), &self.dir)
            .and_then(|(p, raw_version)| self.migrate(p, raw_version))
    }

    /// Đọc một backup cụ thể (người dùng chọn khi project.json hỏng).
    pub fn load_backup(&self, backup: &Path) -> Result<Project, ProjectError> {
        self.fs().check_inside(backup)?;
        Self::load_file(backup, &self.dir).and_then(|(p, v)| self.migrate(p, v))
    }

    fn load_file(path: &Path, dir: &Path) -> Result<(Project, u32), ProjectError> {
        let bytes = std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ProjectError::NotFound,
            _ => e.into(),
        })?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ProjectError::Corrupt {
                detail: e.to_string(),
                backups: list_backups(dir),
            })?;
        let version = value
            .get("schemaVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        if version > SCHEMA_VERSION {
            return Err(ProjectError::NewerSchema {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        let project: Project =
            serde_json::from_value(value).map_err(|e| ProjectError::Corrupt {
                detail: e.to_string(),
                backups: list_backups(dir),
            })?;
        Ok((project, version))
    }

    fn migrate(&self, mut project: Project, from: u32) -> Result<Project, ProjectError> {
        if from < SCHEMA_VERSION {
            // Sao lưu bản gốc trước khi nâng cấp (K-E6).
            let src = self.dir.join(PROJECT_FILE);
            let dst = self
                .dir
                .join(BACKUP_DIR)
                .join(format!("pre-migrate-v{from}.json"));
            self.fs().copy(&src, &dst)?;
            // Chuỗi migration: v0 → v1 (chưa có thay đổi cấu trúc).
            project.schema_version = SCHEMA_VERSION;
        }
        Ok(project)
    }

    /// Lưu an toàn: ghi file tạm → đổi tên. Có `backup = true` thì giữ thêm một bản trong backups/
    /// (đánh dấu là bản lưu tay).
    pub fn save(&self, project: &Project, backup: bool) -> Result<(), ProjectError> {
        self.save_with(project, backup.then_some(BackupKind::Manual))
    }

    /// Như `save`, chọn loại backup: tự lưu nền (`Auto`) hay người dùng lưu (`Manual`).
    pub fn save_with(&self, project: &Project, backup: Option<BackupKind>) -> Result<(), ProjectError> {
        let fs = self.fs();
        let bytes = serde_json::to_vec_pretty(project).map_err(|e| ProjectError::Io {
            detail: e.to_string(),
        })?;
        let path = self.dir.join(PROJECT_FILE);
        let tmp = self.dir.join("project.json.tmp");
        fs.write(&tmp, &bytes)?;
        fs.rename(&tmp, &path)?;
        if let Some(kind) = backup {
            self.write_backup(&bytes, kind)?;
        }
        Ok(())
    }

    fn write_backup(&self, bytes: &[u8], kind: BackupKind) -> Result<(), ProjectError> {
        let fs = self.fs();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let suffix = match kind {
            BackupKind::Manual => ".manual",
            BackupKind::Auto => "",
        };
        fs.write(
            &self.dir.join(BACKUP_DIR).join(format!("project.{stamp}{suffix}.json")),
            bytes,
        )?;
        prune_backups(&self.dir, &fs)
    }

    /// Giữ lại project.json hiện tại thành một backup (trước khi thay bằng bản khác).
    pub fn backup_current(&self) -> Result<(), ProjectError> {
        match std::fs::read(self.dir.join(PROJECT_FILE)) {
            Ok(bytes) => self.write_backup(&bytes, BackupKind::Auto),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn backups(&self) -> Vec<BackupInfo> {
        list_backups(&self.dir)
    }

    pub fn fs(&self) -> ProjectFs<'_> {
        ProjectFs { root: &self.dir }
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.dir.join(CACHE_DIR)
    }
}

impl Drop for ProjectPackage {
    /// Đóng sạch: ghi lại trạng thái để lần mở sau biết không có crash.
    fn drop(&mut self) {
        use std::io::{Seek, Write};
        let _ = self.lock.set_len(0);
        let _ = self.lock.seek(std::io::SeekFrom::Start(0));
        let _ = writeln!(self.lock, "state=closed");
    }
}

/// K-E1: mọi thao tác GHI trong gói đi qua đây. Chỉ cho ghi vào project.json(.tmp), `.lock`,
/// `backups/`, `cache/`. Media gốc và `recordings/` không bao giờ bị ghi.
pub struct ProjectFs<'a> {
    root: &'a Path,
}

impl ProjectFs<'_> {
    fn check_inside(&self, path: &Path) -> Result<(), ProjectError> {
        let rel = path.strip_prefix(self.root).map_err(|_| denied(path))?;
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(denied(path));
        }
        Ok(())
    }

    fn check_writable(&self, path: &Path) -> Result<(), ProjectError> {
        self.check_inside(path)?;
        let rel = path.strip_prefix(self.root).unwrap();
        let first = rel
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or_default();
        let allowed = matches!(
            first,
            PROJECT_FILE | "project.json.tmp" | LOCK_FILE | BACKUP_DIR | CACHE_DIR
        );
        if allowed { Ok(()) } else { Err(denied(path)) }
    }

    pub fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
        self.check_writable(path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    pub fn rename(&self, from: &Path, to: &Path) -> Result<(), ProjectError> {
        self.check_writable(from)?;
        self.check_writable(to)?;
        std::fs::rename(from, to)?;
        Ok(())
    }

    pub fn copy(&self, from: &Path, to: &Path) -> Result<(), ProjectError> {
        self.check_writable(to)?;
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
        Ok(())
    }

    pub fn remove(&self, path: &Path) -> Result<(), ProjectError> {
        self.check_writable(path)?;
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn denied(path: &Path) -> ProjectError {
    ProjectError::WriteOutsideProject {
        path: path.display().to_string(),
    }
}

fn list_backups(dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = std::fs::read_dir(dir.join(BACKUP_DIR)) else {
        return vec![];
    };
    let mut v: Vec<BackupInfo> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("project."))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some(BackupInfo {
                manual: e.file_name().to_string_lossy().ends_with(".manual.json"),
                path: e.path(),
                saved_at: chrono::DateTime::<chrono::Local>::from(modified).to_rfc3339(),
            })
        })
        .collect();
    v.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    v
}

fn prune_backups(dir: &Path, fs: &ProjectFs) -> Result<(), ProjectError> {
    for old in list_backups(dir).into_iter().skip(MAX_BACKUPS) {
        fs.remove(&old.path)?;
    }
    Ok(())
}

#[cfg(windows)]
fn open_exclusive(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(0) // không chia sẻ: process khác mở sẽ bị ERROR_SHARING_VIOLATION
        .open(path)
}

#[cfg(not(windows))]
fn open_exclusive(path: &Path) -> std::io::Result<File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

fn is_sharing_violation(e: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION = 32
    e.raw_os_error() == Some(32)
}
