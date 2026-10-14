//! S02: tìm và khôi phục bản quay dở sau khi app crash / mất điện.
//! Bản quay dùng fragmented MP4 nên phần đã ghi luôn đọc lại được.

use crate::RecorderError;
use cap_recording::recovery::RecoveryManager;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableRecording {
    pub project_dir: PathBuf,
    pub recording_dir: PathBuf,
    pub name: String,
    pub segments: u32,
    pub estimated_duration_ms: f64,
}

/// Quét `<output_root>/*.mochi/recordings/*`.
pub fn list(output_root: &Path) -> Vec<RecoverableRecording> {
    let mut found = Vec::new();
    let Ok(projects) = std::fs::read_dir(output_root) else {
        return found;
    };
    for project in projects.flatten() {
        let project_dir = project.path();
        if project_dir.extension().and_then(|e| e.to_str()) != Some("mochi") {
            continue;
        }
        let Ok(recordings) = std::fs::read_dir(project_dir.join("recordings")) else {
            continue;
        };
        for rec in recordings.flatten() {
            let recording_dir = rec.path();
            if !recording_dir.is_dir() {
                continue;
            }
            if let Some(incomplete) = RecoveryManager::find_incomplete_single(&recording_dir) {
                found.push(RecoverableRecording {
                    project_dir: project_dir.clone(),
                    name: project_dir
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    recording_dir,
                    segments: incomplete.recoverable_segments.len() as u32,
                    estimated_duration_ms: incomplete.estimated_duration.as_secs_f64() * 1000.0,
                });
            }
        }
    }
    found
}

pub fn restore(recording_dir: &Path) -> Result<(), RecorderError> {
    let incomplete =
        RecoveryManager::find_incomplete_single(recording_dir).ok_or(RecorderError::NotRecording)?;
    RecoveryManager::recover(&incomplete).map_err(|e| RecorderError::Internal {
        detail: format!("recover: {e}"),
    })?;
    crate::finalize::cleanup_recovery_workspaces(recording_dir);
    if let Some(name) = crate::finalize::project_dir_of(recording_dir)
        .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
    {
        crate::finalize::set_pretty_name(recording_dir, &name);
    }
    Ok(())
}

pub fn discard(output_root: &Path, recording_dir: &Path) -> Result<(), RecorderError> {
    let project_dir =
        crate::finalize::project_dir_of(recording_dir).ok_or(RecorderError::NotRecording)?;
    crate::finalize::remove_project_dir(output_root, &project_dir).map_err(RecorderError::internal)
}
