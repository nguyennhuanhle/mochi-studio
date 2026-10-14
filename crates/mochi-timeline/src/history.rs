//! Undo/redo trong phiên (E02). Project nhỏ (vài chục KB) nên lưu nguyên bản chụp:
//! đơn giản, không thể lệch giữa thao tác và thao tác ngược.

use mochi_project::Project;

const MAX_STEPS: usize = 300;

#[derive(Default)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
}

impl History {
    /// Ghi lại trạng thái TRƯỚC một thao tác thành công.
    pub fn record(&mut self, before: Project) {
        self.undo.push(before);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: Project) -> Option<Project> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: Project) -> Option<Project> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
