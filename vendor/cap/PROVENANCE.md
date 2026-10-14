# Nguồn gốc code Cap đã vendor

- Upstream: https://github.com/CapSoftware/Cap
- Commit: `f6164c19b9781b439eda79412ba56e9e6d427c53` (2026-10-14)
- License upstream: xem `vendor/cap/LICENSE` (AGPL-3.0; `scap-*` + `cap-camera*` là MIT — `licenses/LICENSE-MIT`)
- Lấy 39 crate trong `crates/` = bao đóng phụ thuộc của `recording`, `editor`, `export`
  (gồm cả 3 crate macOS `camera-avfoundation`, `enc-avfoundation`, `scap-screencapturekit` —
  Cargo cần chúng để resolve, và dùng lại khi làm bản macOS).
- `vendor/wgpu-hal` lấy từ `vendor/wgpu-hal` của Cap (vá theo gfx-rs/wgpu PR 7550).
- `Cargo.lock` gốc của Cap được dùng làm điểm khởi đầu để giữ đúng phiên bản dependency đã kiểm thử.

## Các chỗ đã sửa so với upstream

Mỗi lần sửa code vendor PHẢI ghi thêm vào bảng này.

| File | Thay đổi | Lý do |
|---|---|---|
| `crates/workspace-hack/Cargo.toml` | Bỏ `reqwest`, `tauri-utils`, `clap`, `clap_builder`; bỏ feature `download-binaries`, `copy-dylibs` của `ort-sys` | Không kéo HTTP client (luật K-S1), không tải binary lúc build |
| `crates/recording/src/cursor.rs` | Thêm `KeyboardCaptureFilter` + `set_keyboard_capture_filter()` (cấp process, mặc định `All` = upstream); vòng thu phím chỉ ghi phím qua bộ lọc, phím nhả chỉ ghi nếu phím nhấn đã ghi | Luật K-R3: không thu phím khi chưa opt-in; chế độ "chỉ tổ hợp Ctrl/Alt/Win" |
| `crates/recording/src/studio_recording.rs` | Thêm `set_studio_health_sender()`; cuối `create_segment_pipeline` chuyển tiếp `PipelineHealthEvent` của từng track nếu có sender | Upstream chỉ xuất health ở chế độ Instant; Mochi cần để báo mất thiết bị / rớt frame / đầy ổ khi quay studio |
| `crates/rendering/src/lib.rs` | `pub use frame_pipeline::RenderSession as CapRenderSession`; thêm `render_frame_to_texture()` (giống `produce_frame_with_timings` nhưng trả texture GPU, không readback) | Mochi ghép bản quay đã polish như một lớp trong compositor nhiều track của mình |
| `crates/project/src/configuration.rs` + `crates/rendering/src/lib.rs` | `Camera.border: Option<BorderConfiguration>` (`width` = % cạnh ngắn của webcam); uniforms webcam thường bật `border_*` của shader `composite-video-frame` (shader vốn đã hỗ trợ viền, upstream chỉ dùng cho màn hình) | E08: viền webcam |
| `crates/editor/Cargo.toml` | Bỏ dependency `axum` và `sentry` (không dùng trong mã) | Luật K-S1 / K-S2: không có HTTP server / gửi crash report |
| `crates/workspace-hack/Cargo.toml` | `ort`: `default-features = false` + liệt kê feature cần (bỏ `download-binaries` → kéo `ureq`) | Luật K-S1: không kéo HTTP client |
| Mọi `crates/*/Cargo.toml` | Thêm `license` (MIT cho `scap-*`, `cap-camera*`; AGPL-3.0-only cho phần còn lại) và `publish = false` | `cargo deny check licenses` đọc được license |
| `crates/recording/src/studio_recording.rs` + `capture_pipeline.rs` | `camera_active_max_capture_size` luôn `None`, bỏ hạ 24 fps khi Compatibility + camera; Windows: Compatibility = nửa bitrate (`QUALITY_BPP * 0.5`, `bitrate_multiplier` 0.075) như nhánh macOS | R06: mọi mức chất lượng giữ độ phân giải / fps gốc, "Tiêu chuẩn" chỉ giảm bitrate (upstream Windows: Compatibility = cùng bitrate với Balanced nhưng hạ 1600×1000 / 24 fps khi bật cam) |
| `crates/recording/src/studio_recording.rs` + `output_pipeline/core.rs` | Watcher: trên Windows chỉ track màn hình là bắt buộc; track phụ chết → ghi `track_failures`, gửi `DeviceLost` qua health sender, không hủy các track khác. `Pipeline::stop` (không phải macOS) dừng track phụ bằng `stop_keeping_media()` (giữ phần đã ghi nếu track đã tự kết thúc; chưa ghi được gì → lỗi `TrackEndedWithoutMedia`, không tính là lỗi dọn dẹp). `stop_recording`: chỉ lỗi của track màn hình làm bản quay `Failed`. Resume: bỏ feed mic / cam đã chết ở đoạn trước; tạo đoạn mới lỗi ở "microphone/camera pipeline setup" → thử lại không có nguồn đó | R07: rút mic / webcam khi đang quay → quay tiếp nguồn còn lại, cảnh báo, giữ phần đã quay (upstream Windows: hủy cả bản quay) |
| `crates/enc-ffmpeg/src/video/h264.rs` + `crates/recording/src/output_pipeline/win_fragmented_m4s.rs` | `H264Encoder::codec_name()`; muxer fragmented Windows (màn hình + webcam) gửi `EncoderRebuilt { backend: "libx264" }` qua health sender khi encoder đang dùng là phần mềm | Ca lỗi "không có encoder phần cứng → phần mềm + báo" (upstream chỉ báo ở muxer OOP, Mochi không dùng) |
| `crates/project/src/configuration.rs` + `crates/rendering/src/layers/{click_ripple,cursor}.rs` + `shaders/click-ripple.wgsl` | `CursorConfiguration.highlight: CursorHighlightConfig` (bật, màu, độ đậm, cỡ theo chiều cao con trỏ); `CursorHighlight` dùng lại pipeline ripple (chế độ `params.w = 1`: đĩa sáng tĩnh mép mờ) và được lớp con trỏ vẽ **dưới** sprite, cùng vị trí / độ hiện (ẩn khi con trỏ ẩn hoặc tự ẩn khi đứng yên), cùng `CursorPlacement` (đi theo zoom, cắt theo khung) | Phase 5 R13 / E06: highlight con trỏ kiểu Camtasia (upstream chỉ có ripple khi click) |
| `crates/recording/src/output_pipeline/core.rs` | Lỗi "muxer stopped accepting frames" gắn kiểu `MuxerFailed` | `stop_keeping_media` phân biệt muxer lỗi (bỏ track) với nguồn bị rút (giữ phần đã ghi) |
| `crates/rendering/src/layers/cursor.rs` + `crates/rendering/src/lib.rs` | `CursorLayer` ghi vị trí con trỏ trên khung đầu ra (sau zoom) + độ hiện; `RendererLayers::mochi_cursor()` | E06 v1: Mochi vẽ spotlight / kính lúp quanh con trỏ trong compositor của mình |
| `crates/project/src/configuration.rs` (`BackgroundBlurConfig::removes_background`) + `crates/rendering/src/lib.rs` (`blur_mode_from_config`) + 3 test | Xóa nền webcam bật cả trên Windows (upstream chỉ macOS) | E08 v1: xóa nền AI; ONNX Runtime CPU đi kèm installer |
| `crates/project/src/configuration.rs` (`Camera.chroma_key`, `ChromaKeyConfig`) + `crates/rendering/src/lib.rs` (uniform webcam) + `shaders/composite-video-frame.wgsl` (`chroma_alpha`) | Chroma key webcam: dùng 3 float đệm `_padding1` của uniform (màu khóa đóng gói 24 bit, ngưỡng, độ mềm), so trên mặt phẳng Cb/Cr | E08 v1: phông xanh |
| `crates/scap-targets/src/lib.rs` (`mochi_follow`) + `crates/scap-direct3d/src/lib.rs` (khung cắt dời theo, thêm dependency `scap-targets`) + `crates/cursor-capture/src/position.rs` (`mochi_shifted`) + `crates/recording/src/cursor.rs` | Quay cửa sổ bám theo: độ dời toàn cục (pixel vật lý) do Mochi cập nhật ~30 lần / giây; khung cắt Direct3D và chuẩn hóa vị trí con trỏ cùng dời | R01 v1: bám theo cửa sổ khi bị kéo đi |

Mọi chỗ sửa trong code được đánh dấu `[MOCHI]` (tìm: `grep -rn "\[MOCHI\]" vendor/cap`).

## Cập nhật từ upstream

1. Clone Cap ở commit mới vào thư mục tạm.
2. Chép đè 39 crate, rồi áp lại từng dòng trong bảng trên.
3. Chạy `cargo build -p mochi-cli` và `mochi-cli record` để kiểm tra lại.
