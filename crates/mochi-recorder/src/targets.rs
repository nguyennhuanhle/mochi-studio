//! Liệt kê màn hình / cửa sổ / thiết bị cho màn chọn nguồn (R01, R02).

use crate::{CaptureTarget, RecorderError};
use base64::Engine;
use cap_recording::{MicrophoneFeed, screen_capture::ScreenCaptureTarget};
use scap_targets::{Display, DisplayId, Window, WindowId, bounds::*};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub refresh_rate: u32,
    pub physical_width: f64,
    pub physical_height: f64,
    pub logical_width: f64,
    pub logical_height: f64,
    /// Góc trên-trái theo pixel vật lý trên desktop ảo (để đặt cửa sổ overlay đúng màn hình).
    pub physical_x: f64,
    pub physical_y: f64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: String,
    pub app: String,
    pub title: String,
    /// PNG base64 (data URL) của icon ứng dụng.
    pub icon: Option<String>,
    pub physical_width: f64,
    pub physical_height: f64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CameraInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DeviceList {
    /// Mic mặc định của hệ thống đứng đầu.
    pub mics: Vec<String>,
    pub cameras: Vec<CameraInfo>,
}

pub fn list_displays() -> Vec<DisplayInfo> {
    cap_recording::screen_capture::list_displays()
        .into_iter()
        .map(|(d, display)| {
            let phys = display.physical_size();
            let logical = display.logical_size();
            #[cfg(windows)]
            let pos = display.raw_handle().physical_position();
            #[cfg(not(windows))]
            let pos: Option<PhysicalPosition> = None;
            DisplayInfo {
                id: d.id.to_string(),
                name: d.name,
                refresh_rate: d.refresh_rate,
                physical_width: phys.map_or(0.0, |s| s.width()),
                physical_height: phys.map_or(0.0, |s| s.height()),
                logical_width: logical.map_or(0.0, |s| s.width()),
                logical_height: logical.map_or(0.0, |s| s.height()),
                physical_x: pos.map_or(0.0, |p| p.x()),
                physical_y: pos.map_or(0.0, |p| p.y()),
            }
        })
        .collect()
}

pub fn list_windows() -> Vec<WindowInfo> {
    cap_recording::screen_capture::list_windows()
        .into_iter()
        .filter(|(w, _)| !w.name.trim().is_empty())
        .map(|(w, window)| {
            let phys = window.physical_size();
            WindowInfo {
                id: w.id.to_string(),
                app: w.owner_name,
                title: w.name,
                icon: window.app_icon().map(|png| png_data_url(&png)),
                physical_width: phys.map_or(0.0, |s| s.width()),
                physical_height: phys.map_or(0.0, |s| s.height()),
            }
        })
        .collect()
}

pub fn list_devices() -> DeviceList {
    DeviceList {
        mics: MicrophoneFeed::list().keys().cloned().collect(),
        cameras: cap_camera::list_cameras()
            .map(|c| CameraInfo {
                id: c.device_id().to_string(),
                name: c.display_name().to_string(),
            })
            .collect(),
    }
}

/// Ảnh thu nhỏ (rộng tối đa `max_width`) của màn hình hoặc cửa sổ, dạng data URL JPEG.
pub async fn thumbnail(target: &CaptureTarget, max_width: u32) -> Result<String, RecorderError> {
    let target = resolve_target(target)?;
    let image = cap_recording::screenshot::capture_screenshot(target)
        .await
        .map_err(RecorderError::internal)?;
    let image = image.thumbnail(max_width, max_width);
    let mut buf = std::io::Cursor::new(Vec::new());
    image
        .to_rgb8()
        .write_to(&mut buf, image::ImageFormat::Jpeg)
        .map_err(RecorderError::internal)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    ))
}

pub(crate) fn resolve_target(target: &CaptureTarget) -> Result<ScreenCaptureTarget, RecorderError> {
    match target {
        CaptureTarget::Display { id } => {
            let id: DisplayId = id.parse().map_err(|_| RecorderError::TargetNotFound)?;
            Display::from_id(&id).ok_or(RecorderError::TargetNotFound)?;
            Ok(ScreenCaptureTarget::Display { id })
        }
        CaptureTarget::Window { id } => {
            let id: WindowId = id.parse().map_err(|_| RecorderError::TargetNotFound)?;
            Window::from_id(&id).ok_or(RecorderError::TargetNotFound)?;
            Ok(ScreenCaptureTarget::Window { id })
        }
        CaptureTarget::Area {
            display_id,
            x,
            y,
            width,
            height,
        } => {
            let screen: DisplayId = display_id.parse().map_err(|_| RecorderError::TargetNotFound)?;
            let display = Display::from_id(&screen).ok_or(RecorderError::TargetNotFound)?;
            if *width < 16.0 || *height < 16.0 {
                return Err(RecorderError::TargetNotFound);
            }
            // Vùng phải nằm trong màn hình (đơn vị logic; cho phép lệch 1 px do làm tròn DPI).
            if let Some(size) = display.logical_size()
                && (*x < -1.0 || *y < -1.0 || x + width > size.width() + 1.0 || y + height > size.height() + 1.0)
            {
                return Err(RecorderError::TargetNotFound);
            }
            Ok(ScreenCaptureTarget::Area {
                screen,
                bounds: LogicalBounds::new(
                    LogicalPosition::new(*x, *y),
                    LogicalSize::new(*width, *height),
                ),
            })
        }
    }
}

fn png_data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}
