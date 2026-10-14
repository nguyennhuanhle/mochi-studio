<p align="center">
  <img src="assets/brand/mochi-mark.png" alt="Logo Mochi Studio" width="112">
</p>

<h1 align="center">Mochi Studio</h1>

<p align="center">
  <strong>Quay màn hình, có ngay video đẹp mà không cần chỉnh, rồi biên tập trên timeline thật sự — tất cả chạy trên máy của bạn.</strong>
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-blue"></a>
  <img alt="Nền tảng: Windows 10/11" src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6">
  <img alt="Viết bằng Rust và Tauri" src="https://img.shields.io/badge/built%20with-Rust%20%2B%20Tauri%20v2-orange">
  <img alt="Trạng thái: bản xem trước" src="https://img.shields.io/badge/status-0.1%20preview-yellow">
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>Tiếng Việt</strong>
</p>

<p align="center">
  <img src="assets/screenshots/editor.png" alt="Trình biên tập Mochi Studio: bản quay có nền, bo góc, webcam tròn, dải zoom tự động và vòng sáng quanh con trỏ" width="860">
</p>

Mochi Studio là ứng dụng desktop mã nguồn mở, gộp trình quay màn hình chuyên nghiệp với trình biên tập video, theo tinh thần của **Camtasia**, và bản quay "đẹp sẵn" như **Screen Studio**.

Mỗi nguồn (màn hình, webcam, mic, âm thanh hệ thống) được ghi thành một track riêng. Con trỏ chuột được ghi thành **dữ liệu chứ không dính vào hình**. Nhờ đó zoom, cỡ con trỏ, độ mượt, hiệu ứng click và highlight đều chỉnh được sau khi quay, không phải quay lại.

Mọi thứ chạy trên máy. Không cần tài khoản, không tải lên cloud, không telemetry.

## Mục lục

- [Tính năng](#tính-năng)
- [Trạng thái](#trạng-thái)
- [Bắt đầu](#bắt-đầu)
- [Phím tắt mặc định](#phím-tắt-mặc-định)
- [Build từ mã nguồn](#build-từ-mã-nguồn)
- [Kiến trúc](#kiến-trúc)
- [Quyền riêng tư](#quyền-riêng-tư)
- [Đóng góp](#đóng-góp)
- [Giấy phép](#giấy-phép)
- [Lời cảm ơn](#lời-cảm-ơn)

## Tính năng

### Quay
- **Toàn màn hình, một cửa sổ, hoặc một vùng tùy chọn**: kéo chuột để chọn, nhập đúng số pixel, hoặc dùng preset 16:9 / 9:16 / 1:1 / 4:3.
- **Track riêng** cho màn hình, webcam, mic và âm thanh hệ thống; xem trước webcam và vạch âm lượng trước khi quay.
- **Quay ở độ phân giải gốc**, 30 hoặc 60 fps, ba mức chất lượng; tự dùng encoder phần cứng (NVENC / Quick Sync / AMF), không có thì chuyển sang encoder phần mềm.
- **Phím tắt toàn cục** (đổi được, tự phát hiện trùng phím), đếm ngược 0 / 3 / 5 / 10 giây, tạm dừng / quay tiếp (mỗi đoạn thành một segment), thanh điều khiển nổi nhỏ **không lọt vào bản quay**.
- **Làm nổi con trỏ**: vòng sáng mềm quanh con trỏ (màu, cỡ, độ đậm), có trong trình biên tập và video xuất, không hiện trên màn hình lúc đang quay.
- **Ghi phím bấm khi bạn bật**: mặc định tắt, có chế độ "chỉ phím tắt"; muốn ghi mọi phím phải xác nhận qua cảnh báo.
- **Chịu lỗi tốt**: ghi bằng fragmented MP4 nên khôi phục được sau crash; tự dừng an toàn khi ổ đầy; rút webcam hoặc mic thì vẫn quay tiếp và đánh dấu khoảng bị mất trên timeline.

### Tự làm đẹp
Bấm dừng là project mở thẳng trong trình biên tập, đã làm đẹp sẵn:
- nền (màu, gradient hoặc ảnh), khoảng đệm, bo góc, đổ bóng;
- **zoom tự động** sinh từ các lần click và gõ phím;
- con trỏ di chuyển mượt, to hơn, có vòng sóng khi click, tự ẩn khi đứng yên.

### Biên tập
- Timeline nhiều track: kéo thả, cắt, trim, xóa dồn (ripple delete), bắt dính, khóa track, âm lượng theo track và theo clip, hoàn tác / làm lại không giới hạn.
- Preview realtime **giống hệt video xuất**, vì preview và xuất dùng chung một renderer GPU.
- Khối zoom chỉnh được trên một dải riêng (mức zoom, chuyển động kiểu lò xo, bám theo con trỏ hoặc một điểm cố định).
- Webcam dạng PiP: đặt ở góc hoặc vị trí tự do, hình tròn / bo góc / giữ tỉ lệ gốc, viền, lật gương, ẩn theo từng đoạn.
- Chữ (có thể kèm nền sau chữ), làm mờ và pixelate một vùng để che thông tin nhạy cảm.
- Tỉ lệ khung 16:9, 9:16, 1:1 và 4:3; zoom và webcam tự canh lại.
- Đổi tốc độ clip (0,25×–4×), fade, tách tiếng, waveform.
- Nhập được MP4, MOV, MKV, WebM, MP3, WAV, M4A, PNG, JPG và GIF động.
- Phát hiện media bị thiếu và nối lại theo thư mục, tự tạo proxy cho media 4K, tự lưu mỗi 60 giây và giữ 10 bản sao lưu.

### Xuất
- **MP4** với H.264, thêm HEVC và AV1 nếu GPU hỗ trợ; có mẫu YouTube 1080p, 4K và video dọc 9:16 cho mạng xã hội.
- **GIF** với bảng màu tối ưu, cảnh báo khi GIF quá lớn.
- Xuất cả project hoặc chỉ đoạn đã chọn (in / out).
- Xuất chạy ở **tiến trình riêng**: vẫn sửa tiếp được trong lúc xuất, hủy bất cứ lúc nào, và xếp hàng nhiều lần xuất.

## Trạng thái

Mochi Studio đang ở **bản xem trước (0.1)**. Mọi use case của bản dùng được đầu tiên (MVP) đã làm xong và đã kiểm bằng bản quay thật. App chưa được thử trên nhiều máy, nên có thể còn lỗi. Mong bạn [báo lỗi](../../issues) khi gặp.

- **Hỗ trợ:** Windows 10 (bản 1903 trở lên) và Windows 11, 64-bit.
- **Chưa hỗ trợ:** macOS (dự kiến làm sau, lớp quay đã được tách sẵn) và Linux.
- **Dự kiến tiếp theo:** transition và keyframe, phụ đề bằng Whisper chạy local, khử ồn và chuẩn hóa độ to, hiện phím bấm trong video, callout và mũi tên, xóa nền webcam.

Giao diện có **tiếng Anh** và **tiếng Việt**, mặc định theo ngôn ngữ hệ điều hành.

## Bắt đầu

### Cài đặt
Bộ cài Windows dựng sẵn được đăng ở trang [Releases](../../releases) khi có. Bộ cài NSIS (`*-setup.exe`) cài cho người dùng hiện tại, không cần quyền admin.

Bộ cài chưa được ký số, nên Windows SmartScreen có thể cảnh báo. Bấm **Thông tin thêm → Vẫn chạy**.

### Quay lần đầu
1. Bấm **Quay mới**, chọn màn hình, cửa sổ hoặc vùng, rồi bật các nguồn cần dùng.
2. Nếu muốn, bật **Làm nổi con trỏ**.
3. Bấm **Bắt đầu quay** (hoặc <kbd>F9</kbd>), rồi bấm <kbd>F10</kbd> để dừng.
4. Project đã làm đẹp mở ngay trong trình biên tập. Chỉnh xong thì bấm <kbd>Ctrl</kbd>+<kbd>E</kbd> để xuất.

Bản quay được lưu mặc định ở `Videos\Mochi Studio\`.

## Phím tắt mặc định

| Thao tác | Phím |
|---|---|
| Bắt đầu quay / tạm dừng / quay tiếp | <kbd>F9</kbd> |
| Dừng quay | <kbd>F10</kbd> |
| Hủy bản quay | <kbd>Shift</kbd>+<kbd>F10</kbd> |
| Phát / dừng | <kbd>Space</kbd> |
| Lùi / tiến 1 khung (10 khung khi giữ <kbd>Shift</kbd>) | <kbd>←</kbd> / <kbd>→</kbd> |
| Về đầu | <kbd>Home</kbd> |
| Phóng to / thu nhỏ timeline | <kbd>+</kbd> / <kbd>-</kbd> |
| Cắt tại đầu phát | <kbd>S</kbd> |
| Xóa / xóa và dồn | <kbd>Delete</kbd> / <kbd>Shift</kbd>+<kbd>Delete</kbd> |
| Hoàn tác / làm lại | <kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>Ctrl</kbd>+<kbd>Y</kbd> |
| Đặt điểm vào / ra, bỏ đoạn chọn | <kbd>I</kbd> / <kbd>O</kbd>, <kbd>X</kbd> |
| Xuất | <kbd>Ctrl</kbd>+<kbd>E</kbd> |

Phím tắt khi quay đổi được ở màn thiết lập quay. Nếu một phím đang bị app khác dùng, Mochi Studio sẽ gợi ý một phím còn trống.

## Build từ mã nguồn

### Yêu cầu
- Windows 10 hoặc 11 (x64)
- [Rust](https://rustup.rs/) (stable, 1.85 trở lên; workspace dùng edition 2024)
- Visual Studio 2022 Build Tools, workload **Desktop development with C++**
- **LLVM 19** cho `bindgen`: `winget install LLVM.LLVM --version 19.1.7`
- [Node.js](https://nodejs.org/) 20 trở lên và [pnpm](https://pnpm.io/) 10

### Các bước
```powershell
# 1. Tải FFmpeg 7.1 (shared) và DXC, ghi .cargo/config.toml (chạy lại vô hại)
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1

# 2. Cài thư viện cho giao diện
pnpm install

# 3. Chạy app ở chế độ dev
pnpm dev

# 4. Build bộ cài (NSIS + MSI) vào target/release/bundle/
pnpm build
```

### CLI cho nhà phát triển
`mochi-cli` chạy cùng lõi Rust nhưng không có giao diện. Công cụ này dùng để kiểm thử và không có trong bộ cài.

```powershell
cargo run -p mochi-cli -- targets                          # liệt kê màn hình, cửa sổ, mic, webcam
cargo run -p mochi-cli -- record --secs 5 --system-audio   # quay thử 5 giây
cargo run -p mochi-cli -- --help                           # mọi lệnh (export, render-frame, scenario…)
```

### Kiểm tra
```powershell
cargo check --workspace --tests
pnpm --dir apps/desktop typecheck
cargo deny --manifest-path apps/desktop/src-tauri/Cargo.toml check bans licenses sources
```

## Kiến trúc

```
apps/desktop/          vỏ Tauri v2 (Rust) + giao diện React / TypeScript, i18n Anh / Việt
crates/
  mochi-recorder/      dịch vụ quay: máy trạng thái, kiểm tra trước khi quay, xử lý mất thiết bị
  mochi-project/       định dạng project (project.json), khóa, sao lưu, nâng cấp, nhập media
  mochi-timeline/      thao tác chỉnh sửa thuần, luật timeline, hoàn tác / làm lại
  mochi-editor/        phiên biên tập: mở / lưu, nối lại media, tự lưu
  mochi-render/        compositor GPU nhiều track (wgpu), dùng chung cho preview và xuất
  mochi-preview/       preview native (HWND con + swapchain wgpu), trình phát lấy đồng hồ theo âm thanh
  mochi-export/        xuất MP4 / GIF ở tiến trình con, dò encoder, hàng đợi theo project
  mochi-jobs/          tác vụ nền: proxy, ảnh thu nhỏ, waveform, quản lý bộ nhớ đệm
  mochi-cli/           CLI cho nhà phát triển, để kiểm thử thật
vendor/cap/            các crate lấy từ Cap (quay, encode, vẽ con trỏ), có vá riêng
vendor/wgpu-hal/       wgpu-hal đã vá
```

Một số quyết định thiết kế:
- **Preview vẽ native.** Frame không đi qua IPC; giao diện web chỉ gửi lệnh.
- **Một renderer** tạo cả preview lẫn video xuất, nên thấy gì xuất nấy.
- **Chỉnh sửa không phá hủy.** Bản quay gốc và media đã nhập không bao giờ bị sửa; chỉ `project.json` thay đổi.
- **Phần quay dựa trên [Cap](https://github.com/CapSoftware/Cap).** Các crate của Cap được vendor vào `vendor/cap/`. Mọi chỗ sửa đều đánh dấu `[MOCHI]` trong code và ghi trong [`vendor/cap/PROVENANCE.md`](vendor/cap/PROVENANCE.md).

## Quyền riêng tư

- Mochi Studio không tải media, bản quay hay dữ liệu project lên đâu cả, và chạy được hoàn toàn offline.
- App không có telemetry, không tự gửi báo cáo crash.
- Điều này được kiểm ngay lúc build: [`deny.toml`](deny.toml) cấm HTTP client, HTTP server, thư viện WebSocket và crate telemetry trong cây dependency; `cargo deny` báo lỗi nếu có crate nào lọt vào.
- Ghi phím bấm mặc định tắt; muốn ghi mọi phím phải tự bật và xác nhận qua cảnh báo.

## Đóng góp

Rất hoan nghênh báo lỗi, ý tưởng và pull request. Hãy đọc [CONTRIBUTING.md](CONTRIBUTING.md) trước. Lỗ hổng bảo mật xin báo riêng theo [SECURITY.md](SECURITY.md). Dự án áp dụng [Quy tắc ứng xử](CODE_OF_CONDUCT.md).

## Giấy phép

Mochi Studio dùng giấy phép **GNU Affero General Public License v3.0 only** ([LICENSE](LICENSE)).

Dự án có kèm mã của bên thứ ba theo giấy phép khác:
- các crate của Cap theo AGPL-3.0, riêng `scap-*` và `cap-camera*` theo MIT;
- RNNoise (BSD-3-Clause);
- wgpu-hal đã vá (MIT OR Apache-2.0);
- FFmpeg 7.1, phát hành kèm bản build theo GPL-3.0.

Chi tiết xem [NOTICE](NOTICE) và [THIRD_PARTY.md](THIRD_PARTY.md). Bản cài đặt có các thông báo này trong thư mục `licenses/`.

## Lời cảm ơn

- [Cap](https://github.com/CapSoftware/Cap), dự án mã nguồn mở thay thế Loom; các crate quay, encode và vẽ con trỏ của Cap là nền cho phần quay của Mochi.
- [FFmpeg](https://ffmpeg.org/), [Tauri](https://tauri.app/), [wgpu](https://wgpu.rs/), [gifski](https://gif.ski/), cùng hệ sinh thái Rust và React.
- Camtasia và Screen Studio, vì đã đặt chuẩn cho một công cụ quay màn hình.
