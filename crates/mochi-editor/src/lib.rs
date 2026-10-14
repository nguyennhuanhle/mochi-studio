//! mochi-editor — một phiên biên tập trên một gói `.mochi` (Người biên tập: E01–E04, S01,
//! K-E1, K-E3, K-E4, K-E5, K-E6).
//!
//! Rust giữ project; UI chỉ gửi lệnh và nhận lại `ProjectView` (plan mục 0: "Rust giữ state").

use mochi_project::{
    BackupInfo, MediaItem, MediaKind, Project, ProjectError, ProjectPackage, Ticks, media,
};
use mochi_timeline::{EditOp, History, TimelineError};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// S01: tự lưu mỗi 60 giây khi có thay đổi.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(60);
/// S01: sau một thay đổi lớn (xóa clip / track, import, tách tiếng…) thì lưu sau ngần này.
pub const MAJOR_CHANGE_DELAY: Duration = Duration::from_secs(2);
/// Lỗi tự lưu → thử lại sau ngần này.
pub const AUTOSAVE_RETRY: Duration = Duration::from_secs(10);
/// Tự lưu chỉ giữ thêm một backup mỗi khoảng này: 10 bản giữ lại phủ khoảng 100 phút làm việc
/// thay vì vài phút (Ctrl+S luôn tạo backup).
pub const AUTOSAVE_BACKUP_EVERY: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum EditorError {
    #[error(transparent)]
    Project {
        #[from]
        error: ProjectError,
    },
    #[error(transparent)]
    Timeline {
        #[from]
        error: TimelineError,
    },
    /// K-E4: media đang dùng trên timeline → cần xác nhận.
    #[error("media đang được dùng")]
    #[serde(rename_all = "camelCase")]
    InUse { clip_ids: Vec<String> },
    #[error("không còn gì để hoàn tác/làm lại")]
    NothingToUndo,
}

/// Ảnh chụp trạng thái gửi cho UI sau mỗi thay đổi.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub dir: PathBuf,
    pub project: Project,
    /// id các media đang thiếu file (nhãn "Thiếu media").
    pub missing: Vec<String>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub dirty: bool,
    pub duration: Ticks,
    /// Lần mở trước bị đóng đột ngột: đang dùng bản tự lưu gần nhất.
    pub recovered: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Duplicate {
    pub path: PathBuf,
    pub existing_media_id: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: Vec<String>,
    pub rejected: Vec<Rejected>,
    pub duplicates: Vec<Duplicate>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RelinkReport {
    pub fixed: Vec<String>,
    pub still_missing: Vec<String>,
}

pub struct EditorSession {
    pkg: ProjectPackage,
    project: Project,
    history: History,
    dirty: bool,
    last_save: Instant,
    /// Lần cuối có backup (tay hoặc tự lưu).
    last_backup: Instant,
    /// Thời điểm có thay đổi lớn chưa lưu.
    major_at: Option<Instant>,
    /// Lần tự lưu gần nhất bị lỗi (để chờ AUTOSAVE_RETRY rồi thử lại).
    failed_at: Option<Instant>,
    missing: BTreeSet<String>,
}

impl EditorSession {
    /// Mở gói `.mochi`. Chưa có project.json (gói vừa quay xong ở Vòng 1) → tạo project mới với
    /// các bản quay có sẵn đặt nối tiếp trên track hình đầu tiên.
    pub fn open(dir: &Path) -> Result<Self, EditorError> {
        let dir = &std::path::absolute(dir).map_err(ProjectError::from)?;
        let pkg = ProjectPackage::acquire(dir)?;
        let project = if ProjectPackage::exists(dir) {
            pkg.load()?
        } else {
            let project = project_from_recordings(dir)?;
            pkg.save(&project, true)?;
            project
        };
        Ok(Self::from_parts(pkg, project))
    }

    /// Mở từ một backup (khi project.json hỏng).
    pub fn open_backup(dir: &Path, backup: &Path) -> Result<Self, EditorError> {
        let dir = &std::path::absolute(dir).map_err(ProjectError::from)?;
        let pkg = ProjectPackage::acquire(dir)?;
        let project = pkg.load_backup(backup)?;
        // Giữ project.json hiện tại (vd. bản tự lưu sau crash) thành backup trước khi thay.
        pkg.backup_current()?;
        let mut s = Self::from_parts(pkg, project);
        s.dirty = true;
        s.save(false)?;
        Ok(s)
    }

    pub fn create(dir: &Path, name: &str) -> Result<Self, EditorError> {
        let dir = &std::path::absolute(dir).map_err(ProjectError::from)?;
        if ProjectPackage::exists(dir) {
            return Self::open(dir);
        }
        let pkg = ProjectPackage::acquire(dir)?;
        let project = mochi_project::new_project(name);
        pkg.save(&project, true)?;
        Ok(Self::from_parts(pkg, project))
    }

    fn from_parts(pkg: ProjectPackage, mut project: Project) -> Self {
        // project.json sửa tay / từ bản cũ: kẹp hiệu ứng về giới hạn ngay khi mở.
        for c in &mut project.clips {
            if let mochi_project::ClipContent::Recording { fx } = &mut c.content {
                *fx = fx.clone().clamped();
            }
        }
        let mut s = Self {
            pkg,
            project,
            history: History::default(),
            dirty: false,
            last_save: Instant::now(),
            last_backup: Instant::now(),
            major_at: None,
            failed_at: None,
            missing: BTreeSet::new(),
        };
        s.refresh_missing();
        s
    }

    pub fn dir(&self) -> &Path {
        self.pkg.dir()
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn backups(&self) -> Vec<BackupInfo> {
        self.pkg.backups()
    }

    pub fn view(&self) -> ProjectView {
        ProjectView {
            dir: self.pkg.dir().to_path_buf(),
            project: self.project.clone(),
            missing: self.missing.iter().cloned().collect(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            dirty: self.dirty,
            duration: mochi_timeline::timeline_duration(&self.project),
            recovered: self.pkg.recovered(),
        }
    }

    /// Đường dẫn thật của một media (None = thiếu).
    pub fn media_path(&self, media_id: &str) -> Option<PathBuf> {
        let m = self.project.media.iter().find(|m| m.id == media_id)?;
        media::resolve(self.pkg.dir(), m)
    }

    fn refresh_missing(&mut self) {
        self.missing = self
            .project
            .media
            .iter()
            .filter(|m| media::resolve(self.pkg.dir(), m).is_none())
            .map(|m| m.id.clone())
            .collect();
    }

    fn commit(&mut self, next: Project) {
        let before = std::mem::replace(&mut self.project, next);
        self.history.record(before);
        self.dirty = true;
    }

    /// E02: mọi chỉnh sửa timeline.
    pub fn apply(&mut self, op: &EditOp) -> Result<ProjectView, EditorError> {
        let next = mochi_timeline::apply(&self.project, op)?;
        self.commit(next);
        if matches!(
            op,
            EditOp::DeleteClips { .. }
                | EditOp::RemoveTrack { .. }
                | EditOp::DetachAudio { .. }
                | EditOp::SetCanvas { .. }
                | EditOp::SplitClip { .. }
        ) {
            self.mark_major();
        }
        Ok(self.view())
    }

    pub fn undo(&mut self) -> Result<ProjectView, EditorError> {
        let prev = self
            .history
            .undo(self.project.clone())
            .ok_or(EditorError::NothingToUndo)?;
        self.project = prev;
        self.dirty = true;
        self.refresh_missing();
        Ok(self.view())
    }

    pub fn redo(&mut self) -> Result<ProjectView, EditorError> {
        let next = self
            .history
            .redo(self.project.clone())
            .ok_or(EditorError::NothingToUndo)?;
        self.project = next;
        self.dirty = true;
        self.refresh_missing();
        Ok(self.view())
    }

    /// E01: import nhiều file. File lỗi bị từ chối kèm lý do, file hợp lệ vẫn vào;
    /// file trùng (cùng hash) được báo lại để UI hỏi "dùng lại / thêm bản mới".
    pub fn import(&mut self, paths: &[PathBuf]) -> ImportReport {
        let mut report = ImportReport {
            imported: vec![],
            rejected: vec![],
            duplicates: vec![],
        };
        let mut next = self.project.clone();
        for path in paths {
            match media::probe(path) {
                Ok(item) => {
                    if let Some(existing) = next.media.iter().find(|m| m.hash == item.hash) {
                        report.duplicates.push(Duplicate {
                            path: path.clone(),
                            existing_media_id: existing.id.clone(),
                        });
                        continue;
                    }
                    report.imported.push(item.id.clone());
                    next.media.push(self.with_rel_path(item));
                }
                Err(e) => report.rejected.push(Rejected {
                    path: path.clone(),
                    reason: e.to_string(),
                }),
            }
        }
        if !report.imported.is_empty() {
            self.commit(next);
            self.mark_major();
        }
        report
    }

    /// Người dùng chọn "Thêm bản mới" cho một file trùng.
    pub fn import_duplicate(&mut self, path: &Path) -> Result<ProjectView, EditorError> {
        let item = media::probe(path)?;
        let mut next = self.project.clone();
        next.media.push(self.with_rel_path(item));
        self.commit(next);
        self.mark_major();
        Ok(self.view())
    }

    fn with_rel_path(&self, mut item: MediaItem) -> MediaItem {
        item.rel_path = item
            .abs_path
            .strip_prefix(self.pkg.dir())
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        item
    }

    /// Relink một media sang file mới (người dùng chọn tay).
    pub fn relink(&mut self, media_id: &str, path: &Path) -> Result<ProjectView, EditorError> {
        let is_recording = self
            .project
            .media
            .iter()
            .any(|m| m.id == media_id && m.kind == MediaKind::Recording);
        // Bản quay là một thư mục (layout Cap): chỉ cần có recording-meta.json.
        let probed = if is_recording {
            let dir = std::path::absolute(path).map_err(ProjectError::from)?;
            if !dir.join("recording-meta.json").is_file() {
                return Err(ProjectError::Unsupported { detail: "không phải thư mục bản quay".into() }.into());
            }
            let mut m = self.project.media.iter().find(|m| m.id == media_id).cloned().unwrap();
            m.abs_path = dir;
            m
        } else {
            media::probe(path)?
        };
        let mut next = self.project.clone();
        let m = next
            .media
            .iter_mut()
            .find(|m| m.id == media_id)
            .ok_or(ProjectError::NotFound)?;
        m.abs_path = probed.abs_path;
        m.hash = probed.hash;
        m.size_bytes = probed.size_bytes;
        m.rel_path = None;
        let rel = self.with_rel_path(m.clone()).rel_path;
        m.rel_path = rel;
        self.commit(next);
        self.mark_major();
        self.refresh_missing();
        Ok(self.view())
    }

    /// Relink mọi media thiếu bằng cách tìm theo tên + hash trong một thư mục.
    pub fn relink_folder(&mut self, folder: &Path) -> RelinkReport {
        let mut fixed = vec![];
        let mut next = self.project.clone();
        for m in next.media.iter_mut().filter(|m| self.missing.contains(&m.id)) {
            if let Some(found) = media::find_relink(folder, m) {
                m.abs_path = found;
                m.rel_path = None;
                fixed.push(m.id.clone());
            }
        }
        if !fixed.is_empty() {
            self.commit(next);
            self.mark_major();
            self.refresh_missing();
        }
        RelinkReport {
            fixed,
            still_missing: self.missing.iter().cloned().collect(),
        }
    }

    /// K-E4: media đang có clip trên timeline thì phải `confirm = true` mới xóa (xóa luôn các clip đó).
    pub fn remove_media(&mut self, media_id: &str, confirm: bool) -> Result<ProjectView, EditorError> {
        let in_use: Vec<String> = self
            .project
            .clips
            .iter()
            .filter(|c| c.media_id.as_deref() == Some(media_id))
            .map(|c| c.id.clone())
            .collect();
        if !in_use.is_empty() && !confirm {
            return Err(EditorError::InUse { clip_ids: in_use });
        }
        let mut next = self.project.clone();
        next.clips.retain(|c| c.media_id.as_deref() != Some(media_id));
        next.media.retain(|m| m.id != media_id);
        self.commit(next);
        self.mark_major();
        self.refresh_missing();
        Ok(self.view())
    }

    /// S04/E05: sinh lại vùng zoom tự động cho một clip bản quay. Vùng người dùng đã sửa
    /// (`auto = false`) được giữ nguyên; chỉ thay các vùng tự động.
    pub fn regenerate_zoom(&mut self, clip_id: &str) -> Result<ProjectView, EditorError> {
        let clip = self
            .project
            .clips
            .iter()
            .find(|c| c.id == clip_id)
            .cloned()
            .ok_or(ProjectError::NotFound)?;
        let mochi_project::ClipContent::Recording { fx } = &clip.content else {
            return Err(TimelineError::EffectNotSupported.into());
        };
        let dir = clip
            .media_id
            .as_deref()
            .and_then(|m| self.media_path(m))
            .ok_or(ProjectError::NotFound)?;
        let generated = mochi_project::autozoom::generate(&dir)
            .map_err(|detail| ProjectError::Unsupported { detail })?;
        let mut fx = fx.clone();
        let manual: Vec<_> = fx.zoom.segments.iter().filter(|z| !z.auto).cloned().collect();
        // Vùng tự động mới không được đè lên vùng người dùng đã chỉnh.
        let mut segments: Vec<_> = generated
            .into_iter()
            .filter(|g| !manual.iter().any(|m| m.start < g.end && g.start < m.end))
            .collect();
        segments.extend(manual);
        fx.zoom.segments = segments;
        fx.zoom.auto_error = None;
        self.apply(&EditOp::SetRecordingFx { clip_id: clip_id.to_string(), fx })
    }

    /// Lưu (Ctrl+S, đóng project): `backup` = giữ thêm một bản lưu tay trong backups/.
    pub fn save(&mut self, backup: bool) -> Result<(), EditorError> {
        self.save_with(backup.then_some(mochi_project::BackupKind::Manual))
    }

    fn save_with(&mut self, backup: Option<mochi_project::BackupKind>) -> Result<(), EditorError> {
        self.pkg.save_with(&self.project, backup)?;
        if backup.is_some() {
            self.last_backup = Instant::now();
        }
        self.dirty = false;
        self.last_save = Instant::now();
        self.major_at = None;
        self.failed_at = None;
        Ok(())
    }

    /// S01: thay đổi lớn → tự lưu sớm (sau `MAJOR_CHANGE_DELAY`) thay vì chờ đủ 60 giây.
    pub fn mark_major(&mut self) {
        self.major_at = Some(Instant::now());
    }

    /// S01: gọi định kỳ (mỗi 2 giây); lưu nếu có thay đổi và đã quá `AUTOSAVE_INTERVAL` từ lần lưu
    /// trước, hoặc có thay đổi lớn. Lần trước lỗi → chờ `AUTOSAVE_RETRY` rồi mới thử lại.
    /// Trả về Some(kết quả) nếu đã thử lưu.
    pub fn autosave_if_due(&mut self) -> Option<Result<(), EditorError>> {
        if !self.dirty || self.failed_at.is_some_and(|t| t.elapsed() < AUTOSAVE_RETRY) {
            return None;
        }
        let due = self.last_save.elapsed() >= AUTOSAVE_INTERVAL
            || self.major_at.is_some_and(|t| t.elapsed() >= MAJOR_CHANGE_DELAY);
        if !due {
            return None;
        }
        // Backup tự lưu thưa (AUTOSAVE_BACKUP_EVERY) để 10 bản giữ lại phủ được thời gian dài.
        let backup = (self.last_backup.elapsed() >= AUTOSAVE_BACKUP_EVERY).then_some(mochi_project::BackupKind::Auto);
        let r = self.save_with(backup);
        if r.is_err() {
            self.failed_at = Some(Instant::now());
        }
        Some(r)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.pkg.cache_dir()
    }
}

/// Project mới từ các bản quay có sẵn trong gói (sau Vòng 1): nối tiếp trên "Video 1".
fn project_from_recordings(dir: &Path) -> Result<Project, EditorError> {
    let name = dir
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Mochi")
        .to_string();
    let mut project = mochi_project::new_project(&name);
    let mut cursor = Ticks::ZERO;
    for rec in media::recordings_in(dir) {
        let item = media::probe_recording(dir, &rec)?;
        if let Some(v) = item.video {
            // Canvas theo độ phân giải bản quay đầu tiên (chẵn để encoder chấp nhận).
            if project.media.is_empty() {
                project.canvas.width = v.width & !1;
                project.canvas.height = v.height & !1;
            }
        }
        let id = item.id.clone();
        let dur = item.duration;
        project.media.push(item);
        project = mochi_timeline::apply(
            &project,
            &EditOp::AddClip {
                media_id: id.clone(),
                track_id: Some(project.tracks[0].id.clone()),
                start: cursor,
            },
        )?;
        // R08 + S04: clip mới mang bộ auto-polish mặc định + zoom tự động từ click/gõ phím.
        // Không tạo được zoom → ghi lý do vào clip để UI báo và cho thử lại (không bỏ qua im lặng).
        let zooms = mochi_project::autozoom::generate(&rec);
        let clip = project.clips.iter().find(|c| c.media_id.as_deref() == Some(id.as_str())).cloned();
        if let Some(mochi_project::Clip { id: clip_id, content: mochi_project::ClipContent::Recording { fx }, .. }) = clip {
            let mut fx = fx;
            // R13: highlight con trỏ đã bật ở màn thiết lập quay.
            if let Some(h) = mochi_project::sidecar::RecordingSidecar::read(&rec).cursor_highlight {
                fx.cursor.highlight = h.clamped();
            }
            match zooms {
                Ok(z) => fx.zoom.segments = z,
                Err(e) => {
                    tracing::warn!("autozoom: {e}");
                    fx.zoom.auto_error = Some(e);
                }
            }
            project = mochi_timeline::apply(&project, &EditOp::SetRecordingFx { clip_id, fx })?;
        }
        cursor = cursor + dur;
    }
    Ok(project)
}
