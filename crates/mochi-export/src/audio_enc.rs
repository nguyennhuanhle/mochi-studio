//! Encoder âm thanh chung (AAC trong .m4a, MP3, Opus trong .webm) nhận PCM f32 stereo xen kẽ 48 kHz
//! từ `OfflineMixer` — cùng bộ trộn với preview (S06).

use ffmpeg::{
    ChannelLayout, codec, encoder,
    format::{self, Sample, sample::Type},
    frame,
};

pub struct AudioEncoder {
    enc: encoder::Audio,
    stream: usize,
    rate: i32,
    planar: bool,
    /// Số mẫu mỗi khung encoder (AAC 1024, MP3 1152, Opus 960).
    pub frame_size: usize,
    /// PCM chưa đủ một khung.
    pending: Vec<f32>,
    pts: i64,
}

impl AudioEncoder {
    pub fn open(output: &mut format::context::Output, name: &str, rate: u32, bitrate: u64) -> Result<Self, String> {
        let codec = encoder::find_by_name(name).ok_or_else(|| format!("không có encoder {name}"))?;
        let global = output.format().flags().contains(format::Flags::GLOBAL_HEADER);
        let a = codec.audio().map_err(|e| e.to_string())?;
        // Ưu tiên float phẳng (AAC, MP3), không có thì float xen kẽ (Opus).
        let formats: Vec<Sample> = a.formats().map(|f| f.collect()).unwrap_or_default();
        let planar = formats.contains(&Sample::F32(Type::Planar)) || formats.is_empty();
        let mut e = codec::context::Context::new_with_codec(codec).encoder().audio().map_err(|e| e.to_string())?;
        e.set_rate(rate as i32);
        e.set_channel_layout(ChannelLayout::STEREO);
        e.set_format(if planar { Sample::F32(Type::Planar) } else { Sample::F32(Type::Packed) });
        e.set_bit_rate(bitrate as usize);
        e.set_time_base((1, rate as i32));
        if global {
            e.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let enc = e.open_as(codec).map_err(|e| format!("mở {name}: {e}"))?;
        let stream = {
            let mut st = output.add_stream(codec).map_err(|e| e.to_string())?;
            st.set_parameters(&enc);
            st.set_time_base((1, rate as i32));
            st.index()
        };
        let frame_size = (enc.frame_size() as usize).max(if name == "libopus" { 960 } else { 1024 });
        Ok(Self { enc, stream, rate: rate as i32, planar, frame_size, pending: Vec::new(), pts: 0 })
    }

    fn send(&mut self, pcm: &[f32], n: usize, output: &mut format::context::Output) -> Result<(), String> {
        let fmt = if self.planar { Sample::F32(Type::Planar) } else { Sample::F32(Type::Packed) };
        let mut f = frame::Audio::new(fmt, self.frame_size, ChannelLayout::STEREO);
        f.set_rate(self.rate as u32);
        f.set_samples(n);
        if self.planar {
            for ch in 0..2 {
                for (k, dst) in f.data_mut(ch).chunks_exact_mut(4).take(n).enumerate() {
                    dst.copy_from_slice(&pcm[k * 2 + ch].to_ne_bytes());
                }
            }
        } else {
            for (dst, v) in f.data_mut(0).chunks_exact_mut(4).zip(pcm.iter().take(n * 2)) {
                dst.copy_from_slice(&v.to_ne_bytes());
            }
        }
        f.set_pts(Some(self.pts));
        self.pts += n as i64;
        self.enc.send_frame(&f).map_err(|e| e.to_string())?;
        self.drain(output)
    }

    /// Thêm PCM stereo xen kẽ; đủ khung nào gửi khung đó.
    pub fn push(&mut self, pcm: &[f32], output: &mut format::context::Output) -> Result<(), String> {
        self.pending.extend_from_slice(pcm);
        let block = self.frame_size * 2;
        while self.pending.len() >= block {
            let chunk: Vec<f32> = self.pending.drain(..block).collect();
            self.send(&chunk, self.frame_size, output)?;
        }
        Ok(())
    }

    fn drain(&mut self, output: &mut format::context::Output) -> Result<(), String> {
        let tb = output.stream(self.stream).map(|s| s.time_base()).ok_or("không có luồng tiếng")?;
        let mut pkt = ffmpeg::Packet::empty();
        while self.enc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(self.stream);
            pkt.rescale_ts((1, self.rate), tb);
            pkt.write_interleaved(output).map_err(|e| format!("ghi file: {e}"))?;
        }
        Ok(())
    }

    pub fn finish(&mut self, output: &mut format::context::Output) -> Result<(), String> {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            let n = rest.len() / 2;
            let mut padded = rest;
            padded.resize(self.frame_size * 2, 0.0);
            self.send(&padded, n, output)?;
        }
        self.enc.send_eof().map_err(|e| e.to_string())?;
        self.drain(output)
    }
}
