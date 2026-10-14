//! K-X4: mỗi project chỉ một lần xuất chạy cùng lúc; lần sau vào hàng đợi.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    Queued,
}

/// Hàng đợi thuần (không biết gì về tiến trình): app / CLI gọi `push` khi có job mới,
/// `finish` khi một job kết thúc → nhận job kế tiếp của cùng project để chạy.
pub struct ExportQueue<J> {
    running: HashMap<String, String>,
    waiting: HashMap<String, VecDeque<(String, J)>>,
}

impl<J> Default for ExportQueue<J> {
    fn default() -> Self {
        Self::new()
    }
}

impl<J> ExportQueue<J> {
    pub fn new() -> Self {
        Self { running: HashMap::new(), waiting: HashMap::new() }
    }

    /// Thêm job `job_id` của `project_id`. `Running` → người gọi chạy ngay; `Queued` → chờ.
    pub fn push(&mut self, project_id: &str, job_id: &str, job: J) -> (JobState, Option<J>) {
        if self.running.contains_key(project_id) {
            self.waiting.entry(project_id.into()).or_default().push_back((job_id.into(), job));
            (JobState::Queued, None)
        } else {
            self.running.insert(project_id.into(), job_id.into());
            (JobState::Running, Some(job))
        }
    }

    /// Job `job_id` đã kết thúc → job kế tiếp của cùng project (nếu có) chuyển sang chạy.
    pub fn finish(&mut self, project_id: &str, job_id: &str) -> Option<(String, J)> {
        if self.running.get(project_id).map(String::as_str) != Some(job_id) {
            return None;
        }
        self.running.remove(project_id);
        let next = self.waiting.get_mut(project_id).and_then(VecDeque::pop_front)?;
        self.running.insert(project_id.into(), next.0.clone());
        Some(next)
    }

    /// Bỏ một job đang chờ (hủy trước khi chạy).
    pub fn remove_waiting(&mut self, job_id: &str) -> bool {
        for q in self.waiting.values_mut() {
            if let Some(i) = q.iter().position(|(id, _)| id == job_id) {
                q.remove(i);
                return true;
            }
        }
        false
    }

    pub fn running_of(&self, project_id: &str) -> Option<&str> {
        self.running.get(project_id).map(String::as_str)
    }

    pub fn waiting_count(&self, project_id: &str) -> usize {
        self.waiting.get(project_id).map_or(0, VecDeque::len)
    }
}
