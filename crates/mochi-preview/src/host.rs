//! HWND con (Win32) đặt khớp vùng preview của giao diện web — kết quả Spike B (docs/round-0.md).

/// Hình chữ nhật theo pixel vật lý, tính từ góc trên-trái vùng client của cửa sổ cha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub mod win {
    use super::PhysRect;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOP, RegisterClassW,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow, SW_HIDE,
                SW_SHOWNA, WINDOW_EX_STYLE, WM_ERASEBKGND, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
                WS_VISIBLE,
            },
        },
        core::w,
    };

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_ERASEBKGND {
            return LRESULT(1);
        }
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    /// Phải gọi trên thread sở hữu cửa sổ cha (main thread của Tauri).
    pub fn create_child(parent: isize, r: PhysRect) -> Result<isize, String> {
        unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("MochiPreviewHost");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Mochi preview"),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                r.x,
                r.y,
                r.width.max(1) as i32,
                r.height.max(1) as i32,
                Some(HWND(parent as *mut _)),
                None,
                Some(hinstance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW: {e}"))?;
            SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE).ok();
            Ok(hwnd.0 as isize)
        }
    }

    pub fn move_child(hwnd: isize, r: PhysRect) {
        unsafe {
            SetWindowPos(
                HWND(hwnd as *mut _),
                Some(HWND_TOP),
                r.x,
                r.y,
                r.width.max(1) as i32,
                r.height.max(1) as i32,
                SWP_NOACTIVATE,
            )
            .ok();
        }
    }

    /// Ẩn tạm (khi UI mở hộp thoại đè lên vùng preview).
    pub fn set_visible(hwnd: isize, visible: bool) {
        unsafe {
            let _ = ShowWindow(HWND(hwnd as *mut _), if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }

    pub fn destroy_child(hwnd: isize) {
        unsafe {
            DestroyWindow(HWND(hwnd as *mut _)).ok();
        }
    }
}
