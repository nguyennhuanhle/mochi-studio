//! Dữ liệu ghi kèm trong lúc quay: khoảng mất thiết bị (R07) và cửa sổ đang active (R03).
//! Kết quả ghi ra `mochi-recording.json` khi finalize (xem `mochi_project::sidecar`).

use crate::DeviceKind;
use mochi_project::sidecar::{ActiveWindow, LostRange, LostSource, RecordingSidecar};

#[derive(Default)]
pub(crate) struct ActivityLog {
    data: RecordingSidecar,
    open: Vec<(LostSource, f64)>,
}

fn source_of(kind: DeviceKind) -> Option<LostSource> {
    match kind {
        DeviceKind::Camera => Some(LostSource::Camera),
        DeviceKind::Microphone => Some(LostSource::Microphone),
        DeviceKind::SystemAudio => Some(LostSource::SystemAudio),
        DeviceKind::Screen => None,
    }
}

impl ActivityLog {
    /// Thiết bị mất (`lost`) / có lại tại giây `t`. Báo trùng (nhiều nguồn cùng báo mất) chỉ tính một lần.
    pub fn device(&mut self, kind: DeviceKind, lost: bool, t: f64) {
        let Some(src) = source_of(kind) else { return };
        let pos = self.open.iter().position(|(s, _)| *s == src);
        match (lost, pos) {
            (true, None) => self.open.push((src, t)),
            (false, Some(i)) => {
                let (source, start) = self.open.remove(i);
                if t > start {
                    self.data.lost.push(LostRange { source, start, end: t });
                }
            }
            _ => {}
        }
    }

    /// Ghi lại khi cửa sổ active đổi.
    pub fn window(&mut self, t: f64, app: String, title: String) {
        if self
            .data
            .active_windows
            .last()
            .is_some_and(|w| w.app == app && w.title == title)
        {
            return;
        }
        self.data.active_windows.push(ActiveWindow { at: t, app, title });
    }

    /// Đóng các khoảng còn mở ở cuối bản quay.
    pub fn finish(mut self, total: f64) -> RecordingSidecar {
        for (source, start) in self.open.drain(..) {
            if total > start {
                self.data.lost.push(LostRange { source, start, end: total });
            }
        }
        self.data.lost.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.data
    }
}

/// (tên app, tiêu đề) của cửa sổ đang active.
#[cfg(windows)]
pub(crate) fn foreground_window() -> Option<(String, String)> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut buf) as usize;
        let title = String::from_utf16_lossy(&buf[..n]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut app = String::new();
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut path = [0u16; 520];
            let mut len = path.len() as u32;
            if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(path.as_mut_ptr()), &mut len)
                .is_ok()
            {
                let full = String::from_utf16_lossy(&path[..len as usize]);
                app = full.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
            }
            let _ = CloseHandle(h);
        }
        Some((app, title))
    }
}

#[cfg(not(windows))]
pub(crate) fn foreground_window() -> Option<(String, String)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lost_ranges_open_close_and_dedupe() {
        let mut log = ActivityLog::default();
        log.device(DeviceKind::Camera, true, 2.0);
        log.device(DeviceKind::Camera, true, 2.5); // báo trùng
        log.device(DeviceKind::Microphone, true, 3.0);
        log.device(DeviceKind::Camera, false, 5.0);
        log.device(DeviceKind::Screen, true, 1.0); // không phải nguồn phụ
        let out = log.finish(9.0);
        assert_eq!(
            out.lost,
            vec![
                LostRange { source: LostSource::Camera, start: 2.0, end: 5.0 },
                LostRange { source: LostSource::Microphone, start: 3.0, end: 9.0 },
            ]
        );
    }

    #[test]
    fn active_window_changes_only() {
        let mut log = ActivityLog::default();
        log.window(0.0, "a.exe".into(), "A".into());
        log.window(0.3, "a.exe".into(), "A".into());
        log.window(0.6, "b.exe".into(), "B".into());
        assert_eq!(log.finish(1.0).active_windows.len(), 2);
    }
}
