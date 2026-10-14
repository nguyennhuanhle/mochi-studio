//! Encoder video chung cho H.264 / HEVC / AV1 (phần cứng hoặc phần mềm) + ghi vào muxer FFmpeg.
//! Đầu vào là RGBA của renderer; swscale đổi sang YUV 4:2:0 theo chuẩn BT.709 giới hạn (giống Cap).

use ffmpeg::{
    Dictionary, Rational, codec, color, encoder, format, frame,
    software::scaling::{self, Flags},
};

pub struct VideoEncoder {
    enc: encoder::video::Encoder,
    scaler: scaling::Context,
    rgba: frame::Video,
    yuv: frame::Video,
    stream: usize,
    time_base: Rational,
    pub name: String,
    pub hardware: bool,
}

fn options(name: &str, fps: u32) -> Dictionary<'static> {
    let mut o = Dictionary::new();
    let gop = (fps * 2).to_string();
    match name {
        n if n.ends_with("_nvenc") => {
            o.set("preset", "p5");
            o.set("tune", "hq");
            o.set("rc", "vbr");
            o.set("spatial-aq", "1");
            o.set("g", &gop);
        }
        n if n.ends_with("_qsv") => {
            o.set("preset", "medium");
            o.set("g", &gop);
        }
        n if n.ends_with("_amf") => {
            o.set("quality", "quality");
            o.set("rc", "vbr_peak");
            o.set("g", &gop);
        }
        n if n.ends_with("_mf") => {
            o.set("rate_control", "quality");
            o.set("hw_encoding", "1");
        }
        "libx264" => {
            o.set("preset", "medium");
            o.set("g", &gop);
        }
        "libvpx-vp9" => {
            o.set("deadline", "good");
            o.set("cpu-used", "4");
            o.set("row-mt", "1");
            o.set("auto-alt-ref", "0");
        }
        _ => {}
    }
    o
}

impl VideoEncoder {
    /// Mở encoder `name` cho khung `w`×`h` @ `fps`, thêm luồng vào `output`. Keyframe mỗi 2 giây.
    pub fn open(
        output: &mut format::context::Output,
        name: &str,
        w: u32,
        h: u32,
        fps: u32,
        bitrate: u64,
    ) -> Result<Self, String> {
        Self::open_with_gop(output, name, w, h, fps, bitrate, fps * 2)
    }

    /// Như `open` nhưng chọn khoảng keyframe (proxy dùng GOP ngắn để tua nhanh).
    pub fn open_with_gop(
        output: &mut format::context::Output,
        name: &str,
        w: u32,
        h: u32,
        fps: u32,
        bitrate: u64,
        gop: u32,
    ) -> Result<Self, String> {
        let codec = encoder::find_by_name(name).ok_or_else(|| format!("không có encoder {name}"))?;
        let video = codec.video().map_err(|e| e.to_string())?;
        let pix = video
            .formats()
            .and_then(|mut f| {
                let all: Vec<_> = f.by_ref().collect();
                // X02 (v1): VP9 giữ kênh alpha (WebM nền trong suốt).
                let prefer: &[format::Pixel] =
                    if name == "libvpx-vp9" { &[format::Pixel::YUVA420P, format::Pixel::YUV420P] } else { &[format::Pixel::NV12, format::Pixel::YUV420P] };
                prefer.iter().copied().find(|p| all.contains(p))
            })
            .unwrap_or(format::Pixel::YUV420P);

        let global_header = output.format().flags().contains(format::Flags::GLOBAL_HEADER);
        let mut ctx = codec::context::Context::new_with_codec(codec);
        ctx.set_threading(codec::threading::Config::count(
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
        ));
        let mut e = ctx.encoder().video().map_err(|e| e.to_string())?;
        e.set_width(w);
        e.set_height(h);
        e.set_format(pix);
        let time_base = Rational::new(1, fps as i32);
        e.set_time_base(time_base);
        e.set_frame_rate(Some(Rational::new(fps as i32, 1)));
        e.set_bit_rate(bitrate as usize);
        e.set_max_bit_rate(bitrate as usize * 3 / 2);
        e.set_gop(gop.max(1));
        e.set_colorspace(color::Space::BT709);
        e.set_color_range(color::Range::MPEG);
        unsafe {
            let p = e.as_mut_ptr();
            (*p).color_primaries = ffmpeg::ffi::AVColorPrimaries::AVCOL_PRI_BT709;
            (*p).color_trc = ffmpeg::ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            if global_header {
                (*p).flags |= ffmpeg::ffi::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            }
        }
        let mut opts = options(name, fps);
        opts.set("g", &gop.max(1).to_string());
        let enc = e.open_as_with(codec, opts).map_err(|e| format!("mở {name}: {e}"))?;

        let mut stream = output.add_stream(codec).map_err(|e| e.to_string())?;
        stream.set_time_base(time_base);
        stream.set_rate(Rational::new(fps as i32, 1));
        stream.set_parameters(&enc);
        let stream_index = stream.index();

        let mut scaler = scaling::Context::get(format::Pixel::RGBA, w, h, pix, w, h, Flags::BILINEAR)
            .map_err(|e| e.to_string())?;
        // swscale mặc định BT.601; RGBA là full range, YUV ra là BT.709 giới hạn.
        unsafe {
            let coef = ffmpeg::ffi::sws_getCoefficients(ffmpeg::ffi::SWS_CS_ITU709);
            ffmpeg::ffi::sws_setColorspaceDetails(scaler.as_mut_ptr(), coef, 1, coef, 0, 0, 1 << 16, 1 << 16);
        }
        let mut yuv = frame::Video::new(pix, w, h);
        yuv.set_color_space(color::Space::BT709);
        yuv.set_color_range(color::Range::MPEG);

        Ok(Self {
            enc,
            scaler,
            rgba: frame::Video::new(format::Pixel::RGBA, w, h),
            yuv,
            stream: stream_index,
            time_base,
            name: name.to_string(),
            hardware: crate::caps::is_hardware(name),
        })
    }

    /// Đưa một frame RGBA (liền, `w*4` byte mỗi dòng) có số thứ tự `index` vào encoder.
    pub fn push_rgba(
        &mut self,
        rgba: &[u8],
        index: i64,
        output: &mut format::context::Output,
    ) -> Result<(), String> {
        let (w, h) = (self.rgba.width() as usize, self.rgba.height() as usize);
        let stride = self.rgba.stride(0);
        let dst = self.rgba.data_mut(0);
        for row in 0..h {
            dst[row * stride..row * stride + w * 4].copy_from_slice(&rgba[row * w * 4..(row + 1) * w * 4]);
        }
        self.scaler.run(&self.rgba, &mut self.yuv).map_err(|e| e.to_string())?;
        self.yuv.set_pts(Some(index));
        self.enc.send_frame(&self.yuv).map_err(|e| format!("{}: {e}", self.name))?;
        self.drain(output)
    }

    fn drain(&mut self, output: &mut format::context::Output) -> Result<(), String> {
        let mut packet = ffmpeg::Packet::empty();
        loop {
            match self.enc.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(self.stream);
                    packet.rescale_ts(self.time_base, output.stream(self.stream).unwrap().time_base());
                    packet.write_interleaved(output).map_err(|e| format!("ghi file: {e}"))?;
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => return Ok(()),
                Err(ffmpeg::Error::Eof) => return Ok(()),
                Err(e) => return Err(format!("{}: {e}", self.name)),
            }
        }
    }

    pub fn finish(&mut self, output: &mut format::context::Output) -> Result<(), String> {
        self.enc.send_eof().map_err(|e| e.to_string())?;
        self.drain(output)
    }

    /// Mở thật encoder `name` ở 256×256 và mã hóa vài frame vào bộ nhớ (null muxer).
    pub fn self_test(name: &str) -> Result<(), String> {
        let mut out = format::output_as("NUL", "null").map_err(|e| e.to_string())?;
        let mut enc = Self::open(&mut out, name, 256, 256, 30, 1_000_000)?;
        out.write_header().map_err(|e| e.to_string())?;
        let rgba = vec![128u8; 256 * 256 * 4];
        for i in 0..3 {
            enc.push_rgba(&rgba, i, &mut out)?;
        }
        enc.finish(&mut out)?;
        out.write_trailer().map_err(|e| e.to_string())?;
        Ok(())
    }
}
