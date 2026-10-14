//! K-R1: chỉ một instance Mochi Studio cho mỗi phiên đăng nhập Windows.
//! Instance thứ hai không có `RecorderService` riêng: nó đưa cửa sổ chính đang chạy lên trước rồi thoát.

/// Giữ mutex suốt vòng đời tiến trình (Windows tự nhả khi tiến trình kết thúc, kể cả khi crash).
pub struct Guard(#[allow(dead_code)] isize);

/// `Some(guard)` nếu đây là instance đầu tiên; `None` nếu đã có instance khác (đã đưa nó lên trước).
#[cfg(windows)]
pub fn acquire() -> Option<Guard> {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    // "Local\": theo phiên đăng nhập. Tên cố định cho cả bản dev lẫn bản cài.
    let handle = match unsafe { CreateMutexW(None, false, w!("Local\\MochiStudio.SingleInstance")) } {
        Ok(h) => h,
        Err(e) => {
            // Không tạo được mutex (rất hiếm) → không chặn khởi động; service vẫn chặn quay trùng trong tiến trình.
            tracing::warn!("Không tạo được mutex một-instance: {e}");
            return Some(Guard(0));
        }
    };
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Some(Guard(handle.0 as isize));
    }
    tracing::info!("Mochi Studio đã chạy → đưa cửa sổ chính lên trước và thoát");
    focus_existing();
    None
}

/// Cửa sổ chính là cửa sổ "Mochi Studio" duy nhất có mặt trên taskbar (các overlay đều `skip_taskbar`
/// → WS_EX_TOOLWINDOW). Khi đang quay nó bị thu nhỏ nhưng có content protection, nên mở lại không lọt vào bản quay.
#[cfg(windows)]
fn focus_existing() {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW, GetWindowTextW, IsIconic, SetForegroundWindow, ShowWindow,
        GWL_EXSTYLE, SW_RESTORE, SW_SHOW, WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn visit(hwnd: HWND, out: LPARAM) -> BOOL {
        let mut buf = [0u16; 64];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        if String::from_utf16_lossy(&buf[..n]) != "Mochi Studio" {
            return true.into();
        }
        let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return true.into();
        }
        unsafe { *(out.0 as *mut isize) = hwnd.0 as isize };
        false.into()
    }

    let mut found: isize = 0;
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut isize as isize));
    }
    if found == 0 {
        return;
    }
    let hwnd = HWND(found as *mut _);
    unsafe {
        let _ = ShowWindow(hwnd, if IsIconic(hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(not(windows))]
pub fn acquire() -> Option<Guard> {
    Some(Guard(0))
}
