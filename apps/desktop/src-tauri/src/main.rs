// Ẩn console ở bản release trên Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Worker tokio cần stack lớn như main thread (xem build.rs).
const RUNTIME_STACK_SIZE: usize = 16 * 1024 * 1024;

fn main() {
    // Tiến trình con xuất video (X04): cùng file exe, không mở cửa sổ.
    if let Some(job) = mochi_export::job_arg() {
        let code = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(RUNTIME_STACK_SIZE)
            .build()
            .map(|rt| rt.block_on(mochi_export::child_main(&job)))
            .unwrap_or(1);
        std::process::exit(code);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(RUNTIME_STACK_SIZE)
        .build()
        .expect("không tạo được tokio runtime")
        .block_on(mochi_desktop_lib::run());
}
