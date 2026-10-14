//! R01 (v1): quay cửa sổ "bám theo" khi cửa sổ bị kéo đi.
//!
//! Một thread nhẹ đọc vị trí cửa sổ ~30 lần / giây và ghi độ dời so với lúc bắt đầu đoạn quay hiện
//! tại vào `scap_targets::mochi_follow`; khung cắt của bản quay (vá `[MOCHI]` trong scap-direct3d)
//! và chuẩn hóa vị trí con trỏ (vá trong cap-recording) dời theo. Kích thước vùng quay giữ nguyên;
//! vùng không ra khỏi màn hình đang quay.

use scap_targets::{Window, WindowId};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

pub struct Follow {
    stop: Arc<AtomicBool>,
    base: Arc<Mutex<Option<(f64, f64)>>>,
    handle: Option<JoinHandle<()>>,
}

fn window_pos(id: &WindowId) -> Option<(f64, f64)> {
    let b = Window::from_id(id)?.raw_handle().physical_bounds()?;
    Some((b.position().x(), b.position().y()))
}

impl Follow {
    pub fn start(window_id: &str) -> Option<Self> {
        let id: WindowId = window_id.parse().ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let base = Arc::new(Mutex::new(window_pos(&id)));
        scap_targets::mochi_follow::set(0, 0);
        let handle = std::thread::Builder::new()
            .name("mochi-follow-window".into())
            .spawn({
                let (stop, base) = (stop.clone(), base.clone());
                move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Some((x, y)) = window_pos(&id) {
                            let mut b = base.lock().unwrap();
                            let (bx, by) = *b.get_or_insert((x, y));
                            scap_targets::mochi_follow::set((x - bx).round() as i32, (y - by).round() as i32);
                        }
                        std::thread::sleep(Duration::from_millis(33));
                    }
                }
            })
            .ok()?;
        Some(Self { stop, base, handle: Some(handle) })
    }

    /// Đoạn quay mới (quay tiếp sau tạm dừng) cắt lại theo vị trí cửa sổ hiện tại → độ dời về 0.
    pub fn rebase(&self) {
        *self.base.lock().unwrap() = None;
        scap_targets::mochi_follow::set(0, 0);
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        scap_targets::mochi_follow::set(0, 0);
    }
}
