struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_inv: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var texture: texture_2d<f32>;
@group(1) @binding(1)
var texture_sampler: sampler;

struct SpriteSheet {
    padding: vec2<f32>,
    box_size: vec2<f32>,
    image_size: vec2<f32>,
    num_cols: u32,
    mask_color: u32,
}

@group(2) @binding(0)
var<uniform> sprite_sheet: SpriteSheet;

struct Vertex {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct Instance {
    @location(2) pos: vec3<f32>,
    @location(3) scale: vec2<f32>,
    @location(4) sprite_index: u32,
    @location(5) color_flip: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) mask_oklab: vec3<f32>,
}

// cube root
fn cbrt(x: f32) -> f32 {
    var y = sign(x) * bitcast<f32>(bitcast<u32>(abs(x)) / 3u + 0x2a514067u);

    // newton iters
    for (var i = 0; i < 1; i++) {
        y = (2. * y + x / (y * y)) * .333333333;
    }

    // halley iters
    for (var i = 0; i < 1; i++) {
        let y3 = y * y * y;
        y *= (y3 + 2. * x) / (2. * y3 + x);
    }

    return y;
}

fn linear_srgb_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708f * c.r + 0.5363325363f * c.g + 0.0514459929f * c.b;
    let m = 0.2119034982f * c.r + 0.6806995451f * c.g + 0.1073969566f * c.b;
    let s = 0.0883024619f * c.r + 0.2817188376f * c.g + 0.6299787005f * c.b;

    let l_ = cbrt(l);
    let m_ = cbrt(m);
    let s_ = cbrt(s);

    return vec3(
        0.2104542553f * l_ + 0.7936177850f * m_ - 0.0040720468f * s_,
        1.9779984951f * l_ - 2.4285922050f * m_ + 0.4505937099f * s_,
        0.0259040371f * l_ + 0.7827717662f * m_ - 0.8086757660f * s_,
    );
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return (1.0 - t) * a + t * b;
}

fn lerp_vec3(a: vec3<f32>, b: vec3<f32>, t: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        lerp(a.x, b.x, t.x),
        lerp(a.y, b.y, t.y),
        lerp(a.z, b.z, t.z)
    );
}

fn lerp_vec2(a: vec2<f32>, b: vec2<f32>, t: vec2<f32>) -> vec2<f32> {
    return (1.0 - t) * a + t * b;
}

fn inv_lerp(a: f32, b: f32, val: f32) -> f32 {
    return (val - a) / (b - a);
}

fn inv_lerp_vec2(a: vec2<f32>, b: vec2<f32>, val: vec2<f32>) -> vec2<f32> {
    return vec2(inv_lerp(a.x, b.x, val.x), inv_lerp(a.y, b.y, val.y));
}

fn parse_rgb(c: u32) -> vec3<f32> {
    return vec3<f32>(
        f32((c >> 16) & 0xFF) / 255.0,
        f32((c >> 8) & 0xFF) / 255.0,
        f32(c & 0xFF) / 255.0,
    );
}

@vertex
fn vs_main(
    model: Vertex,
    instance: Instance,
) -> VertexOutput {
    var out: VertexOutput;
    out.color = parse_rgb(instance.color_flip >> 8);
    let mask = parse_rgb(sprite_sheet.mask_color);
    if dot(mask, mask) != 0 {
        out.mask_oklab = linear_srgb_to_oklab(mask);
    }

    let row: u32 = instance.sprite_index / sprite_sheet.num_cols;
    let col: u32 = instance.sprite_index - sprite_sheet.num_cols * row;

    let offset = sprite_sheet.box_size.xy * vec2<f32>(f32(col), f32(row)) + sprite_sheet.padding;

    var uv = model.uv;
    let flip = instance.color_flip & 0xFF;
    if flip != 0u {
        uv.x = 1.0 - uv.x;
    }
    let box_uv = lerp_vec2(vec2(0.0), sprite_sheet.box_size - sprite_sheet.padding * 2.0, uv) + offset;
    let total_uv = inv_lerp_vec2(vec2(0.0), sprite_sheet.image_size, box_uv);
    out.uv = total_uv;

    // billboarding
    let scale_x = instance.scale.x;
    var pos = vec4<f32>(instance.pos, 1.0);
    let up: vec4<f32> = camera.view_inv[1];
    let right: vec4<f32> = camera.view_inv[0];

    pos += right * model.pos.x * scale_x;
    pos += up * model.pos.y * instance.scale.y;

    out.clip_position = camera.view_proj * pos;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(texture, texture_sampler, in.uv);
    if color.a < 0.2 {
        discard;
    }
    // mask the color with the instance color
    if dot(in.mask_oklab, in.mask_oklab) != 0 {
        let color_oklab = linear_srgb_to_oklab(color.rgb);
        let t = length(color_oklab - in.mask_oklab) / length(in.mask_oklab);
        let rgb = lerp_vec3(in.color, color.rgb, vec3(t));
        color = vec4<f32>(rgb, color.a);
    }
    return color;
}
