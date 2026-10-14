//! R10 (v1): chỉ thu tiếng của ứng dụng đang quay (quay cửa sổ).
//!
//! Dùng WASAPI process loopback (vá `[MOCHI]` trong cap-recording): tiếng của cả cây tiến trình
//! sở hữu cửa sổ, nên thông báo / nhạc của app khác không lọt vào. Cần Windows 10 2004 (build 19041)
//! trở lên; máy cũ hơn hoặc mở không được → dùng âm thanh hệ thống như thường và báo cảnh báo.

/// Build Windows tối thiểu có process loopback.
pub const MIN_BUILD: u32 = 19041;

#[cfg(windows)]
pub fn os_build() -> u32 {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;
    let mut buf = [0u16; 32];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return 0;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n]).trim().parse().unwrap_or(0)
}

#[cfg(not(windows))]
pub fn os_build() -> u32 {
    0
}

pub fn supported() -> bool {
    os_build() >= MIN_BUILD
}

/// PID của tiến trình sở hữu cửa sổ `window_id`.
#[cfg(windows)]
pub fn window_pid(window_id: &str) -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let id: scap_targets::WindowId = window_id.parse().ok()?;
    let window = scap_targets::Window::from_id(&id)?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window.raw_handle().inner(), Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

#[cfg(not(windows))]
pub fn window_pid(_window_id: &str) -> Option<u32> {
    None
}

/// Đặt nguồn tiếng cho phiên sắp quay. Trả về true nếu đã bật thu tiếng riêng app.
pub fn arm(window_id: Option<&str>) -> bool {
    #[cfg(windows)]
    {
        use cap_recording::screen_capture::mochi_app_audio as m;
        use std::sync::atomic::Ordering;
        m::MOCHI_APP_AUDIO_FELL_BACK.store(false, Ordering::Relaxed);
        let pid = window_id.filter(|_| supported()).and_then(window_pid).unwrap_or(0);
        m::MOCHI_APP_AUDIO_PID.store(pid, Ordering::Relaxed);
        pid != 0
    }
    #[cfg(not(windows))]
    {
        let _ = window_id;
        false
    }
}

/// Pipeline đã quay về âm thanh hệ thống vì không mở được process loopback.
pub fn fell_back() -> bool {
    #[cfg(windows)]
    {
        cap_recording::screen_capture::mochi_app_audio::MOCHI_APP_AUDIO_FELL_BACK
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(windows))]
    false
}

pub fn disarm() {
    #[cfg(windows)]
    cap_recording::screen_capture::mochi_app_audio::MOCHI_APP_AUDIO_PID
        .store(0, std::sync::atomic::Ordering::Relaxed);
}
