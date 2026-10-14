fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Pipeline WGPU/MediaFoundation/FFmpeg của Cap đệ quy sâu: stack 1 MiB mặc định của
        // main thread không đủ (giống Cap desktop). Xem docs/round-0.md phát hiện #6.
        println!("cargo:rustc-link-arg-bin=mochi-desktop=/STACK:16777216");
    }
    tauri_build::build()
}
