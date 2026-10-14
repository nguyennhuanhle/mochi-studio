//! E10 (v1): hình khối, mũi tên, đường kẻ, callout — vẽ bằng tiny-skia ra ảnh RGBA (alpha thẳng),
//! theo đúng độ phân giải khung đầu ra để nét sắc khi xuất 4K.

use crate::overlays::{TextRasterizer, parse_rgba};
use mochi_project::ShapeKind;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

/// Ảnh đã vẽ + vị trí tâm của nó trên khung (pixel).
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub center: [f32; 2],
}

fn paint(hex: &str) -> Paint<'static> {
    let [r, g, b, a] = parse_rgba(hex);
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

fn finish(pm: Pixmap, center: [f32; 2]) -> Raster {
    let (width, height) = (pm.width(), pm.height());
    // tiny-skia lưu premultiplied → đổi về alpha thẳng cho shader.
    let rgba = pm
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Raster { width, height, rgba, center }
}

/// Hình khối từ (x0, y0) tới (x1, y1) (chuẩn hóa 0–1) trên khung `cw`×`ch`. `scale` = tỉ lệ khung
/// xuất / canvas (độ dày nét tính theo pixel canvas).
#[allow(clippy::too_many_arguments)]
pub fn shape(
    kind: ShapeKind,
    (x0, y0, x1, y1): (f64, f64, f64, f64),
    stroke: &str,
    stroke_width: f64,
    fill: Option<&str>,
    cw: u32,
    ch: u32,
    scale: f32,
) -> Option<Raster> {
    let sw = (stroke_width as f32 * scale).max(0.5);
    let (ax, ay, bx, by) = (x0 as f32 * cw as f32, y0 as f32 * ch as f32, x1 as f32 * cw as f32, y1 as f32 * ch as f32);
    let head = (sw * 4.5).max(12.0 * scale);
    let pad = sw + head + 4.0;
    let (minx, miny) = (ax.min(bx) - pad, ay.min(by) - pad);
    let (w, h) = (((ax - bx).abs() + pad * 2.0).ceil() as u32, ((ay - by).abs() + pad * 2.0).ceil() as u32);
    let mut pm = Pixmap::new(w.clamp(1, 8192), h.clamp(1, 8192))?;
    let (ax, ay, bx, by) = (ax - minx, ay - miny, bx - minx, by - miny);
    let st = Stroke { width: sw, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    let tr = Transform::identity();
    match kind {
        ShapeKind::Rectangle | ShapeKind::Ellipse => {
            let rect = Rect::from_ltrb(ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))?;
            let path = if kind == ShapeKind::Rectangle {
                let r = (sw * 1.5).min(rect.width() / 2.0).min(rect.height() / 2.0);
                rounded_rect(rect, r)?
            } else {
                PathBuilder::from_oval(rect)?
            };
            if let Some(f) = fill {
                pm.fill_path(&path, &paint(f), FillRule::Winding, tr, None);
            }
            pm.stroke_path(&path, &paint(stroke), &st, tr, None);
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let (dx, dy) = (bx - ax, by - ay);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            let (ux, uy) = (dx / len, dy / len);
            // Mũi tên: thân dừng trước đầu nhọn để đầu không bị nét tròn làm cùn.
            let end = if kind == ShapeKind::Arrow { (bx - ux * head * 0.6, by - uy * head * 0.6) } else { (bx, by) };
            let mut pb = PathBuilder::new();
            pb.move_to(ax, ay);
            pb.line_to(end.0, end.1);
            pm.stroke_path(&pb.finish()?, &paint(stroke), &st, tr, None);
            if kind == ShapeKind::Arrow {
                let (px, py) = (-uy, ux);
                let base = (bx - ux * head, by - uy * head);
                let wing = head * 0.55;
                let mut pb = PathBuilder::new();
                pb.move_to(bx, by);
                pb.line_to(base.0 + px * wing, base.1 + py * wing);
                pb.line_to(base.0 - px * wing, base.1 - py * wing);
                pb.close();
                pm.fill_path(&pb.finish()?, &paint(stroke), FillRule::Winding, tr, None);
            }
        }
    }
    Some(finish(pm, [minx + w as f32 / 2.0, miny + h as f32 / 2.0]))
}

fn rounded_rect(r: Rect, rad: f32) -> Option<tiny_skia::Path> {
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    let k = 0.5523 * rad;
    let mut pb = PathBuilder::new();
    pb.move_to(l + rad, t);
    pb.line_to(rt - rad, t);
    pb.cubic_to(rt - rad + k, t, rt, t + rad - k, rt, t + rad);
    pb.line_to(rt, b - rad);
    pb.cubic_to(rt, b - rad + k, rt - rad + k, b, rt - rad, b);
    pb.line_to(l + rad, b);
    pb.cubic_to(l + rad - k, b, l, b - rad + k, l, b - rad);
    pb.line_to(l, t + rad);
    pb.cubic_to(l, t + rad - k, l + rad - k, t, l + rad, t);
    pb.close();
    pb.finish()
}

/// Callout: hộp chữ bo góc tâm (cx, cy) (pixel khung) + đuôi tam giác chỉ vào (tx, ty).
#[allow(clippy::too_many_arguments)]
pub fn callout(
    text_raster: &mut TextRasterizer,
    text: &str,
    font: &str,
    size_px: f32,
    color: &str,
    background: &str,
    (cx, cy): (f32, f32),
    (tx, ty): (f32, f32),
    max_width: f32,
) -> Option<Raster> {
    let (tw, th, trgba) = text_raster.rasterize(text, font, size_px, color, None, max_width);
    let pad = (size_px * 0.6).max(6.0);
    let (bw, bh) = (tw as f32 + pad * 2.0, th as f32 + pad * 2.0);
    let (bl, bt) = (cx - bw / 2.0, cy - bh / 2.0);
    let margin = 4.0;
    let minx = bl.min(tx) - margin;
    let miny = bt.min(ty) - margin;
    let maxx = (bl + bw).max(tx) + margin;
    let maxy = (bt + bh).max(ty) + margin;
    let mut pm = Pixmap::new(((maxx - minx).ceil() as u32).clamp(1, 8192), ((maxy - miny).ceil() as u32).clamp(1, 8192))?;
    let tr = Transform::identity();
    let bg = paint(background);
    let rect = Rect::from_xywh(bl - minx, bt - miny, bw, bh)?;
    pm.fill_path(&rounded_rect(rect, (size_px * 0.45).max(6.0).min(bw / 2.0).min(bh / 2.0))?, &bg, FillRule::Winding, tr, None);
    // Đuôi: đáy nằm ở tâm hộp, rộng ~35% cạnh ngắn, nhọn ở điểm chỉ.
    let (bcx, bcy) = (cx - minx, cy - miny);
    let (px, py) = (tx - minx, ty - miny);
    let (dx, dy) = (px - bcx, py - bcy);
    let len = (dx * dx + dy * dy).sqrt();
    if len > bw.min(bh) * 0.5 {
        let (nx, ny) = (-dy / len, dx / len);
        let half = bw.min(bh) * 0.18;
        let mut pb = PathBuilder::new();
        pb.move_to(bcx + nx * half, bcy + ny * half);
        pb.line_to(px, py);
        pb.line_to(bcx - nx * half, bcy - ny * half);
        pb.close();
        pm.fill_path(&pb.finish()?, &bg, FillRule::Winding, tr, None);
    }
    // Chữ (alpha thẳng) → pixmap premultiplied rồi vẽ đè.
    let mut text_pm = Pixmap::new(tw.max(1), th.max(1))?;
    for (dst, src) in text_pm.pixels_mut().iter_mut().zip(trgba.chunks_exact(4)) {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    pm.draw_pixmap(
        (bl - minx + pad).round() as i32,
        (bt - miny + pad).round() as i32,
        text_pm.as_ref(),
        &PixmapPaint::default(),
        tr,
        None,
    );
    Some(finish(pm, [(minx + maxx) / 2.0, (miny + maxy) / 2.0]))
}
