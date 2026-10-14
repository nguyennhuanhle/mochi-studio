//! X05 / E13 (v1): đọc / ghi phụ đề SRT và WebVTT.

use mochi_project::{Caption, Ticks};

fn stamp(t: Ticks, sep: char) -> String {
    let ms = (t.secs().max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn to_srt(captions: &[Caption]) -> String {
    let mut s = String::new();
    for (i, c) in captions.iter().enumerate() {
        s.push_str(&format!("{}\r\n{} --> {}\r\n{}\r\n\r\n", i + 1, stamp(c.start, ','), stamp(c.end, ','), c.text.trim()));
    }
    s
}

pub fn to_vtt(captions: &[Caption]) -> String {
    let mut s = String::from("WEBVTT\n\n");
    for c in captions {
        s.push_str(&format!("{} --> {}\n{}\n\n", stamp(c.start, '.'), stamp(c.end, '.'), c.text.trim()));
    }
    s
}

/// "hh:mm:ss,mmm" / "hh:mm:ss.mmm" / "mm:ss.mmm" → giây.
fn parse_stamp(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, sec] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        [m, sec] => (0.0, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Đọc SRT hoặc WebVTT (tự nhận). Bỏ qua khối hỏng; thẻ định dạng (<i>, {\an8}) bị gỡ.
pub fn parse(text: &str) -> Vec<Caption> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let mut it = lines[ti].split("-->");
        let (Some(a), Some(b)) = (it.next(), it.next()) else { continue };
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_stamp(a), parse_stamp(b)) else { continue };
        let body: String = lines[ti + 1..].join("\n");
        let mut clean = String::new();
        let mut in_tag = false;
        let mut in_brace = false;
        for ch in body.chars() {
            match ch {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                '{' => in_brace = true,
                '}' if in_brace => in_brace = false,
                _ if !in_tag && !in_brace => clean.push(ch),
                _ => {}
            }
        }
        if end <= start || clean.trim().is_empty() {
            continue;
        }
        out.push(Caption {
            id: format!("cap-{}", out.len() + 1),
            start: Ticks::from_secs(start),
            end: Ticks::from_secs(end),
            text: clean.trim().to_string(),
            words: vec![],
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_and_vtt_round_trip() {
        let caps = vec![
            Caption { id: "a".into(), start: Ticks::from_secs(1.5), end: Ticks::from_secs(3.25), text: "Xin chào".into(), words: vec![] },
            Caption { id: "b".into(), start: Ticks::from_secs(3661.0), end: Ticks::from_secs(3662.004), text: "dòng hai\ncó xuống dòng".into(), words: vec![] },
        ];
        let srt = to_srt(&caps);
        assert!(srt.contains("00:00:01,500 --> 00:00:03,250"));
        assert!(srt.contains("01:01:01,000 --> 01:01:02,004"));
        let back = parse(&srt);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].text, "dòng hai\ncó xuống dòng");
        assert!((back[0].start.secs() - 1.5).abs() < 1e-6);
        let vtt = to_vtt(&caps);
        assert!(vtt.starts_with("WEBVTT"));
        assert_eq!(parse(&vtt).len(), 2);
    }

    #[test]
    fn parse_strips_tags_and_skips_broken_blocks() {
        let caps = parse("1\n00:00:01,000 --> 00:00:02,000\n<i>nghiêng</i> {\\an8}trên\n\n2\nhỏng\n\n3\n00:00:05.000 --> 00:00:04.000\nngược\n");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].text, "nghiêng trên");
    }
}
