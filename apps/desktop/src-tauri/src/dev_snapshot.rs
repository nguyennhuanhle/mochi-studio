//! Tự chụp giao diện (chỉ bản dev): bật bằng `MOCHI_UI_SNAPSHOT=<thư mục kết quả>`.
//!
//! Dùng PrintWindow(PW_RENDERFULLCONTENT) để cửa sổ TỰ VẼ vào bitmap — không chụp màn hình,
//! nên không lộ cửa sổ khác của người dùng và không cần đưa Mochi lên trước.

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Chỉ bản dev: bản release luôn None → mọi lệnh tự chụp trả lỗi / không làm gì (không tắt được
/// chống chụp của cửa sổ phụ, không giả lập bấm × trên máy người dùng).
fn out_dir() -> Option<PathBuf> {
    mochi_project::test_env("MOCHI_UI_SNAPSHOT").map(PathBuf::from)
}

#[tauri::command]
#[specta::specta]
pub fn dev_snapshot_enabled() -> bool {
    out_dir().is_some()
}

/// Kịch bản tự chụp: "recorder" (mặc định) hoặc "editor"; kèm thư mục media tổng hợp.
#[tauri::command]
#[specta::specta]
pub fn dev_snapshot_mode() -> (String, String) {
    (
        mochi_project::test_env("MOCHI_UI_SNAPSHOT_MODE").unwrap_or_else(|| "recorder".into()),
        out_dir().map(|d| d.display().to_string()).unwrap_or_default(),
    )
}

/// Chụp cửa sổ `label` → `<dir>/<name>.png`.
#[tauri::command]
#[specta::specta]
pub fn dev_snapshot(app: AppHandle, label: String, name: String) -> Result<String, String> {
    let dir = out_dir().ok_or("snapshot chưa bật")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("không có cửa sổ {label}"))?;
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    // Cửa sổ phụ khi quay được loại khỏi mọi API chụp (R07) nên PrintWindow ra màu đen.
    // Bản dev tạm tắt bảo vệ đúng lúc chụp để xem được giao diện, rồi bật lại ngay.
    let protected = label != crate::recorder_app::MAIN;
    if protected {
        window.set_content_protected(false).ok();
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let captured = win::print_window(hwnd);
    if protected {
        window.set_content_protected(true).ok();
    }
    let (w, h, rgba) = captured?;
    let path = dir.join(format!("{name}.png"));
    image::save_buffer(&path, &rgba, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Giả lập người dùng bấm × ở cửa sổ chính (phát CloseRequested như thật).
#[tauri::command]
#[specta::specta]
pub fn dev_snapshot_close_main(app: AppHandle) -> Result<(), String> {
    out_dir().ok_or("snapshot chưa bật")?;
    let main = app
        .get_webview_window(crate::recorder_app::MAIN)
        .ok_or("không có cửa sổ chính")?;
    main.close().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn dev_snapshot_finish(app: AppHandle, report: String) -> Result<(), String> {
    if let Some(dir) = out_dir() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("report.json"), report).map_err(|e| e.to_string())?;
    }
    // Thoát như khi người dùng đóng cửa sổ (đóng project sạch).
    let _ = crate::editor_app::save_on_exit(&app);
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
mod win {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
        },
        Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
        UI::WindowsAndMessaging::GetWindowRect,
    };

    /// PW_RENDERFULLCONTENT: vẽ cả nội dung DirectComposition (WebView2).
    const PW_RENDERFULLCONTENT: u32 = 2;

    pub fn print_window(hwnd: isize) -> Result<(u32, u32, Vec<u8>), String> {
        unsafe {
            let hwnd = HWND(hwnd as *mut _);
            let mut r = RECT::default();
            GetWindowRect(hwnd, &mut r).map_err(|e| e.to_string())?;
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp.into());
            let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT));
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            if !ok.as_bool() {
                return Err("PrintWindow thất bại".into());
            }
            for px in bgra.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            Ok((w as u32, h as u32, bgra))
        }
    }
}
