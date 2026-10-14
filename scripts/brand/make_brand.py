"""Tạo tài sản thương hiệu từ logo gốc (nền trắng):
- assets/brand/mochi-logo.png : logo đầy đủ (hình + chữ), nền trong suốt
- assets/brand/mochi-mark.png : chỉ hình máy quay, vuông 1024×1024, nền trong suốt (nguồn cho icon app)
- assets/brand/preview-*.png  : bản ghép trên nền sáng / tối để kiểm bằng mắt

Nền trắng được tách bằng "color to alpha" chỉ trong vùng nối liền với mép ảnh (flood fill), nên
phần trắng bên trong hình (mặt mochi) giữ nguyên.

Chạy: python scripts/brand/make_brand.py <ảnh logo gốc>
"""

import sys
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "brand"
TOL = 26  # khoảng cách tối đa tới màu nền để còn coi là nền / bóng đổ nhạt
D0, D1 = 2, 26  # dải chuyển từ trong suốt hoàn toàn → đục hoàn toàn
SHADOW_TOL = 140  # bóng đổ: nhạt tới mức này…
SHADOW = (28, 22, 48)  # màu bóng (tím rất tối)
SHADOW_SAT = 22  # …và gần như không màu (max-min kênh)


def dist(c, bg):
    return max(abs(c[0] - bg[0]), abs(c[1] - bg[1]), abs(c[2] - bg[2]))


def remove_background(im: Image.Image) -> Image.Image:
    """Loang từ mép ảnh qua nền trắng và bóng đổ.
    - Điểm ít màu (nền, bóng đổ xám): coi là bóng tối trong suốt — màu SHADOW, độ đục theo độ tối.
      Trên nền tối bóng tự chìm, trên nền sáng trông y như ảnh gốc.
    - Điểm có màu ở mép hình (khử răng cưa): "color to alpha" so với nền trắng.
    Viền có màu của hình chặn loang, nên phần trắng bên trong hình (mặt mochi, thanh timeline) giữ nguyên."""
    rgb = im.convert("RGB")
    w, h = rgb.size
    px = rgb.load()
    bg = px[2, 2]
    bg_lum = sum(bg) / 3
    sh_lum = sum(SHADOW) / 3
    out = rgb.convert("RGBA")
    op = out.load()
    seen = bytearray(w * h)
    q = deque()
    for x in range(w):
        q.extend([(x, 0), (x, h - 1)])
    for y in range(h):
        q.extend([(0, y), (w - 1, y)])
    while q:
        x, y = q.popleft()
        i = y * w + x
        if seen[i]:
            continue
        seen[i] = 1
        c = px[x, y]
        d = dist(c, bg)
        sat = max(c) - min(c)
        if sat <= SHADOW_SAT and d <= SHADOW_TOL:
            a = min(1.0, max(0.0, (bg_lum - sum(c) / 3) / (bg_lum - sh_lum)))
            alpha = round(a * 255)
            op[x, y] = (*SHADOW, alpha) if alpha >= 3 else (0, 0, 0, 0)
        elif d <= TOL:
            a = min(1.0, max(0.0, (d - D0) / (D1 - D0)))
            fg = tuple(max(0, min(255, round((c[k] - (1 - a) * bg[k]) / max(a, 1e-6)))) for k in range(3))
            op[x, y] = (*fg, round(a * 255))
        else:
            continue
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= nx < w and 0 <= ny < h and not seen[ny * w + nx]:
                q.append((nx, ny))
    return out


def opaque_rows(im: Image.Image, thresh=40):
    a = im.getchannel("A")
    w, h = im.size
    data = a.load()
    return [any(data[x, y] > thresh for x in range(0, w, 2)) for y in range(h)]


def main(src: str):
    OUT.mkdir(parents=True, exist_ok=True)
    logo = remove_background(Image.open(src))
    logo = logo.crop(logo.getbbox())
    logo.save(OUT / "mochi-logo.png", optimize=True)

    # Hình máy quay = khối hàng đục đầu tiên (chữ nằm bên dưới, cách một khoảng trống).
    rows = opaque_rows(logo)
    y = 0
    while y < len(rows) and not rows[y]:
        y += 1
    start = y
    gap = 0
    while y < len(rows):
        gap = gap + 1 if not rows[y] else 0
        if gap > 20:
            break
        y += 1
    mark = logo.crop((0, start, logo.width, y - gap))
    mark = mark.crop(mark.getbbox())
    side = max(mark.size)
    pad = round(side * 0.04)
    canvas = Image.new("RGBA", (side + 2 * pad, side + 2 * pad), (0, 0, 0, 0))
    canvas.paste(mark, ((canvas.width - mark.width) // 2, (canvas.height - mark.height) // 2), mark)
    canvas.resize((1024, 1024), Image.LANCZOS).save(OUT / "mochi-mark.png", optimize=True)

    for name, color in (("light", (247, 245, 242)), ("dark", (24, 23, 26))):
        for kind, img in (("mark", canvas), ("logo", logo)):
            bgimg = Image.new("RGBA", img.size, (*color, 255))
            bgimg.alpha_composite(img)
            bgimg.convert("RGB").resize((img.width // 2, img.height // 2)).save(OUT / f"preview-{kind}-{name}.png")
    print("logo", logo.size, "mark", mark.size)


if __name__ == "__main__":
    main(sys.argv[1])
