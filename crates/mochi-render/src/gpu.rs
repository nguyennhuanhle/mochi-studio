//! Một wgpu device DX12 dùng chung cho compositor, decoder (zero-copy D3D11) và renderer của Cap.

use anyhow::Context;

#[derive(Clone)]
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    pub fn new() -> anyhow::Result<Self> {
        // Dùng DXC đi kèm exe (dxcompiler.dll) như Cap: biên dịch shader của renderer Cap nhanh hơn
        // FXC mặc định rất nhiều (FXC làm lần mở renderer đầu tiên mất 7–30 s). Thiếu DLL → lùi FXC.
        let instance = cap_rendering::create_wgpu_instance_sync();
        // Kiểm thử máy yếu: MOCHI_TEST_FORCE_SOFTWARE_GPU=1 → dùng GPU phần mềm (WARP).
        let force_software = mochi_project::test_env("MOCHI_TEST_FORCE_SOFTWARE_GPU").is_some();
        let adapter = pollster_block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: force_software,
            compatible_surface: None,
        }))
        .context("không tìm thấy GPU hỗ trợ DX12")?;
        // Bật các feature mà renderer / converter của Cap dùng khi adapter hỗ trợ.
        let wanted = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::PIPELINE_CACHE;
        let (device, queue) = pollster_block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mochi-device"),
            required_features: adapter.features() & wanted,
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .context("không tạo được wgpu device")?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    pub fn adapter_name(&self) -> String {
        self.adapter.get_info().name
    }

    /// Có đạt yêu cầu tối thiểu không: GPU phần cứng chạy DirectX 12.
    pub fn check(&self) -> GpuCheck {
        let info = self.adapter.get_info();
        let software = info.device_type == wgpu::DeviceType::Cpu
            || info.name.contains("Basic Render")
            || info.name.contains("WARP");
        let driver = format!("{} {}", info.driver, info.driver_info).trim().to_string();
        let old_driver = !software && driver_too_old(&driver);
        GpuCheck {
            ok: !software && !old_driver,
            adapter: info.name,
            driver,
            software,
            old_driver,
        }
    }

    /// Thử tạo GPU; không có GPU DX12 nào → `GpuCheck` báo lỗi (app vẫn cho quay).
    pub fn probe() -> GpuCheck {
        match Self::new() {
            Ok(g) => g.check(),
            Err(e) => GpuCheck { ok: false, adapter: format!("{e:#}"), driver: String::new(), software: false, old_driver: false },
        }
    }
}

/// Kết quả kiểm GPU lúc khởi động.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GpuCheck {
    pub ok: bool,
    pub adapter: String,
    pub driver: String,
    /// GPU phần mềm (WARP / Basic Render Driver): chạy được nhưng rất chậm.
    pub software: bool,
    /// Driver cũ hơn mức tối thiểu (WDDM 2.6, xem `MIN_DRIVER_MAJOR`).
    pub old_driver: bool,
}

/// Phiên bản driver hiển thị trên Windows có số đầu = đời WDDM (26 = WDDM 2.6 / Windows 10 1903,
/// 31 = WDDM 3.1…). Dưới mức này coi là driver quá cũ cho DirectX 12 + decoder phần cứng.
pub const MIN_DRIVER_MAJOR: u32 = 26;

/// `driver` = chuỗi từ wgpu (vd. "32.0.15.9174"). Không đọc được số → không kết luận là cũ.
/// Kiểm thử: `MOCHI_TEST_GPU_DRIVER=<chuỗi>` thay cho driver thật.
pub fn driver_too_old(driver: &str) -> bool {
    let fake = mochi_project::test_env("MOCHI_TEST_GPU_DRIVER");
    let driver = fake.as_deref().unwrap_or(driver);
    driver
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|t| t.matches('.').count() >= 2)
        .and_then(|v| v.split('.').next()?.parse::<u32>().ok())
        .is_some_and(|major| major < MIN_DRIVER_MAJOR)
}

#[cfg(test)]
mod tests {
    #[test]
    fn driver_major_threshold() {
        assert!(!super::driver_too_old("32.0.15.9174"));
        assert!(!super::driver_too_old("26.20.100.7262"));
        assert!(super::driver_too_old("25.20.100.6618"));
        assert!(super::driver_too_old("Intel 23.21.13.8813"));
        assert!(!super::driver_too_old(""));
    }
}

/// Chạy future ngắn (khởi tạo wgpu) ngay trên thread hiện tại.
fn pollster_block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context as TaskCx, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop_raw() -> RawWaker {
        fn clone(_: *const ()) -> RawWaker {
            noop_raw()
        }
        fn noop(_: *const ()) {}
        static VT: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VT)
    }
    let waker = unsafe { Waker::from_raw(noop_raw()) };
    let mut cx = TaskCx::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}
