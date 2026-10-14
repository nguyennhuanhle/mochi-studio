//! Shader ghép lớp của Mochi (v1): một quad có vị trí / kích thước / xoay theo pixel canvas,
//! bo góc (SDF, khử răng cưa), viền, chỉnh màu (sáng / tương phản / bão hòa), mặt nạ wipe,
//! tối dần (fade qua đen), cộng các chế độ vẽ không cần ảnh: đổ bóng, hình chữ nhật màu, spotlight.

pub const QUAD_SHADER: &str = r#"
struct U {
    a: vec4<f32>, // tâm.x, tâm.y, nửa rộng, nửa cao (pixel canvas, y hướng xuống)
    b: vec4<f32>, // canvas.w, canvas.h, xoay (rad), độ mờ
    c: vec4<f32>, // uv_max.x, uv_max.y, chế độ, bán kính bo góc (px)
    d: vec4<f32>, // viền (px) | elip (spotlight), sáng, tương phản, bão hòa (−1…1)
    e: vec4<f32>, // wipe (phần hiện 0–1), hướng wipe (0 tắt, 1 trái→phải, 2 phải→trái), tối (0–1), lề (px, cho bóng)
    f: vec4<f32>, // màu viền
    g: vec4<f32>, // màu tô (hình chữ nhật / bóng / spotlight)
    h: vec4<f32>, // spotlight: x0, y0, x1, y1 (px)
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0));
    let ext = u.a.zw + vec2<f32>(u.e.w);
    let local = corners[i] * ext;
    let cs = cos(u.b.z);
    let sn = sin(u.b.z);
    let p = u.a.xy + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    var o: VsOut;
    o.pos = vec4<f32>(p.x / u.b.x * 2.0 - 1.0, 1.0 - p.y / u.b.y * 2.0, 0.0, 1.0);
    o.local = local;
    return o;
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let rr = min(r, min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(rr);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - rr;
}

fn adjust(rgb_in: vec3<f32>) -> vec3<f32> {
    var rgb = rgb_in + vec3<f32>(u.d.y * 0.4);
    rgb = (rgb - vec3<f32>(0.5)) * (1.0 + u.d.z) + vec3<f32>(0.5);
    let l = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(l), rgb, 1.0 + u.d.w);
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * (1.0 - u.e.z);
}

fn wipe_mask(local: vec2<f32>) -> f32 {
    if (u.e.y < 0.5) { return 1.0; }
    let frac = local.x / (2.0 * u.a.z) + 0.5;
    let edge = 1.5 / max(2.0 * u.a.z, 1.0);
    if (u.e.y < 1.5) { return 1.0 - smoothstep(u.e.x - edge, u.e.x + edge, frac); }
    return smoothstep(1.0 - u.e.x - edge, 1.0 - u.e.x + edge, frac);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let mode = u.c.z;
    let half = u.a.zw;
    let opacity = u.b.w;
    if (mode < 0.5) {
        // Ảnh: bo góc + viền + chỉnh màu.
        let sdf = rounded_box(in.local, half, u.c.w);
        let inside = clamp(0.5 - sdf, 0.0, 1.0);
        if (inside <= 0.0) { discard; }
        let uv = (in.local / half) * 0.5 + vec2<f32>(0.5);
        let half_texel = 0.5 / vec2<f32>(textureDimensions(tex));
        let c = textureSample(tex, samp, min(uv * u.c.xy, u.c.xy - half_texel));
        var rgb = adjust(c.rgb);
        var a = c.a;
        if (u.d.x > 0.0) {
            let t = clamp(sdf + u.d.x + 0.5, 0.0, 1.0) * u.f.a;
            rgb = mix(rgb, u.f.rgb, t);
            a = mix(a, 1.0, t);
        }
        return vec4<f32>(rgb, a * inside * opacity * wipe_mask(in.local));
    }
    if (mode < 1.5) {
        // Đổ bóng mềm dưới khung (lệch xuống một chút).
        let blur = max(u.e.w, 1.0);
        let sdf = rounded_box(in.local - vec2<f32>(0.0, blur * 0.25), half, u.c.w);
        let a = (1.0 - smoothstep(-blur * 0.3, blur, sdf)) * u.g.a;
        return vec4<f32>(u.g.rgb, a * opacity);
    }
    if (mode < 2.5) {
        // Hình chữ nhật bo góc tô màu (khung thiết bị, thanh trình duyệt…), có viền.
        let sdf = rounded_box(in.local, half, u.c.w);
        let inside = clamp(0.5 - sdf, 0.0, 1.0);
        if (inside <= 0.0) { discard; }
        var col = u.g;
        if (u.d.x > 0.0) {
            let t = clamp(sdf + u.d.x + 0.5, 0.0, 1.0);
            col = mix(col, u.f, t);
        }
        return vec4<f32>(col.rgb, col.a * inside * opacity * wipe_mask(in.local));
    }
    // Spotlight: làm tối mọi thứ ngoài vùng sáng (chữ nhật hoặc elip), mép mềm.
    let p = u.a.xy + in.local;
    let c0 = (u.h.xy + u.h.zw) * 0.5;
    let hh = max((u.h.zw - u.h.xy) * 0.5, vec2<f32>(1.0));
    var sdf = 0.0;
    if (u.d.x > 0.5) {
        sdf = (length((p - c0) / hh) - 1.0) * min(hh.x, hh.y);
    } else {
        sdf = rounded_box(p - c0, hh, u.c.w);
    }
    let feather = max(u.c.w, 2.0);
    let a = smoothstep(-feather * 0.5, feather, sdf) * u.g.a;
    return vec4<f32>(u.g.rgb, a * opacity);
}
"#;

/// Chế độ vẽ của quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Image = 0,
    Shadow = 1,
    Solid = 2,
    Spotlight = 3,
}

/// Tham số một quad (đơn vị pixel của khung đầu ra, y hướng xuống).
#[derive(Debug, Clone, Copy)]
pub struct Quad {
    pub center: [f32; 2],
    pub half: [f32; 2],
    pub rotation: f32,
    pub opacity: f32,
    pub uv_max: [f32; 2],
    pub mode: Mode,
    pub radius: f32,
    pub border: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub wipe: f32,
    pub wipe_dir: f32,
    pub dim: f32,
    pub extent: f32,
    pub border_color: [f32; 4],
    pub fill: [f32; 4],
    pub spot: [f32; 4],
}

impl Quad {
    pub fn new(mode: Mode, center: [f32; 2], half: [f32; 2]) -> Self {
        Self {
            center,
            half,
            rotation: 0.0,
            opacity: 1.0,
            uv_max: [1.0, 1.0],
            mode,
            radius: 0.0,
            border: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            wipe: 1.0,
            wipe_dir: 0.0,
            dim: 0.0,
            extent: 0.0,
            border_color: [1.0; 4],
            fill: [0.0, 0.0, 0.0, 1.0],
            spot: [0.0; 4],
        }
    }

    pub fn uniform(&self, canvas: (u32, u32)) -> [f32; 32] {
        let s = self;
        [
            s.center[0], s.center[1], s.half[0], s.half[1],
            canvas.0 as f32, canvas.1 as f32, s.rotation, s.opacity,
            s.uv_max[0], s.uv_max[1], s.mode as u8 as f32, s.radius,
            s.border, s.brightness, s.contrast, s.saturation,
            s.wipe, s.wipe_dir, s.dim, s.extent,
            s.border_color[0], s.border_color[1], s.border_color[2], s.border_color[3],
            s.fill[0], s.fill[1], s.fill[2], s.fill[3],
            s.spot[0], s.spot[1], s.spot[2], s.spot[3],
        ]
    }
}

/// "#RRGGBB(AA)" → RGBA 0–1.
pub fn color(hex: &str) -> [f32; 4] {
    let [r, g, b, a] = crate::overlays::parse_rgba(hex);
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}
