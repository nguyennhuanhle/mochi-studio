//! E10: overlay chữ (cosmic-text, font hệ thống, dấu tiếng Việt chuẩn) — vẽ ra ảnh RGBA một lần
//! rồi cache theo nội dung.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

pub struct TextRasterizer {
    fonts: FontSystem,
    swash: SwashCache,
}

impl TextRasterizer {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            swash: SwashCache::new(),
        }
    }

    /// Vẽ `text` (nhiều dòng được) thành ảnh RGBA. Có `background` thì vẽ khung bo góc phía sau.
    pub fn rasterize(
        &mut self,
        text: &str,
        font: &str,
        size_px: f32,
        color: &str,
        background: Option<&str>,
        max_width: f32,
    ) -> (u32, u32, Vec<u8>) {
        let text = if text.is_empty() { " " } else { text };
        self.rasterize_spans(&[(text, color)], font, size_px, color, background, max_width, false)
    }

    /// Như `rasterize` nhưng mỗi đoạn một màu (phụ đề động tô chữ đang nói); `center` = căn giữa dòng.
    #[allow(clippy::too_many_arguments)]
    pub fn rasterize_spans(
        &mut self,
        spans: &[(&str, &str)],
        font: &str,
        size_px: f32,
        color: &str,
        background: Option<&str>,
        max_width: f32,
        center: bool,
    ) -> (u32, u32, Vec<u8>) {
        let metrics = Metrics::new(size_px, size_px * 1.25);
        let mut buffer = Buffer::new(&mut self.fonts, metrics);
        buffer.set_size(&mut self.fonts, Some(max_width), None);
        let base = Attrs::new().family(Family::Name(font));
        let rich: Vec<(&str, Attrs)> = spans
            .iter()
            .map(|(t, c)| {
                let [r, g, b, a] = parse_rgba(c);
                (*t, base.clone().color(Color::rgba(r, g, b, a)))
            })
            .collect();
        let align = center.then_some(cosmic_text::Align::Center);
        buffer.set_rich_text(&mut self.fonts, rich, &base, Shaping::Advanced, align);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let measure = |buffer: &Buffer| {
            let (mut tw, mut th) = (0f32, 0f32);
            for run in buffer.layout_runs() {
                tw = tw.max(run.line_w);
                th = th.max(run.line_top + run.line_height);
            }
            (tw, th)
        };
        let (mut tw, mut th) = measure(&buffer);
        if center {
            // Căn giữa trong đúng bề rộng của chữ (không phải `max_width`) để ảnh không thừa lề.
            buffer.set_size(&mut self.fonts, Some(tw.ceil().max(1.0)), None);
            buffer.shape_until_scroll(&mut self.fonts, false);
            (tw, th) = measure(&buffer);
        }
        let pad = if background.is_some() { (size_px * 0.4).round() } else { 2.0 };
        let (w, h) = ((tw + pad * 2.0).ceil().max(1.0) as u32, (th + pad * 2.0).ceil().max(1.0) as u32);
        let mut img = vec![0u8; (w * h * 4) as usize];

        if let Some(bg) = background {
            let [r, g, b, a] = parse_rgba(bg);
            let radius = (size_px * 0.3).max(4.0);
            for y in 0..h {
                for x in 0..w {
                    if inside_rounded(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32, radius) {
                        let i = ((y * w + x) * 4) as usize;
                        img[i..i + 4].copy_from_slice(&[r, g, b, a]);
                    }
                }
            }
        }

        let [r, g, b, a] = parse_rgba(color);
        let text_color = Color::rgba(r, g, b, a);
        buffer.draw(&mut self.fonts, &mut self.swash, text_color, |x, y, gw, gh, c| {
            for dy in 0..gh as i32 {
                for dx in 0..gw as i32 {
                    let px = x + dx + pad as i32;
                    let py = y + dy + pad as i32;
                    if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                        continue;
                    }
                    blend(&mut img, ((py as u32 * w + px as u32) * 4) as usize, c);
                }
            }
        });
        (w, h, img)
    }
}

impl Default for TextRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Trộn một điểm màu (alpha thẳng) lên ảnh RGBA.
fn blend(img: &mut [u8], i: usize, c: Color) {
    let sa = c.a() as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    let da = img[i + 3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    for (k, s) in [c.r(), c.g(), c.b()].into_iter().enumerate() {
        let d = img[i + k] as f32;
        img[i + k] = ((s as f32 * sa + d * da * (1.0 - sa)) / out_a.max(1e-6)).round() as u8;
    }
    img[i + 3] = (out_a * 255.0).round() as u8;
}

fn inside_rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
    let cx = x.clamp(r, w - r);
    let cy = y.clamp(r, h - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// "#RRGGBB" hoặc "#RRGGBBAA".
pub fn parse_rgba(s: &str) -> [u8; 4] {
    let s = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255);
    match s.len() {
        8 => [byte(0), byte(2), byte(4), byte(6)],
        6 => [byte(0), byte(2), byte(4), 255],
        _ => [255, 255, 255, 255],
    }
}
