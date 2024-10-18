struct Vertex {
    @builtin(vertex_index) vertex_index: u32,
}

struct Instance {
    @location(0) xywh: vec4<u32>,
    @location(1) color: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return (1.0 - t) * a + t * b;
}

fn lerp_vec2(a: vec2<f32>, b: vec2<f32>, t: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(lerp(a.x, b.x, t.x), lerp(a.y, b.y, t.y));
}

fn inv_lerp(a: f32, b: f32, val: f32) -> f32 {
    return (val - a) / (b - a);
}

fn inv_lerp_vec2(a: vec2<f32>, b: vec2<f32>, val: vec2<f32>) -> vec2<f32> {
    return vec2(inv_lerp(a.x, b.x, val.x), inv_lerp(a.y, b.y, val.y));
}

@vertex
fn vs_main(
    model: Vertex,
    instance: Instance,
) -> VertexOutput {
    var out: VertexOutput;
    let c = instance.color;
    let xywh = instance.xywh;
    out.color = vec4<f32>(
        f32((c >> 24) & 0xFF) / 255.0,
        f32((c >> 16) & 0xFF) / 255.0,
        f32((c >> 8) & 0xFF) / 255.0,
        f32((c >> 0) & 0xFF) / 255.0,
    );

    var u = f32((model.vertex_index << 1) & 2);
    var v = f32(model.vertex_index & 2);
    let flip = model.vertex_index > 2;
    if flip {
        v = 2.0 - v;
    }

    var uv = vec2<f32>(u, v);
    out.uv = uv * 0.5;
    uv *= 0.2;
    uv += 0.4;
    out.clip_position = vec4<f32>(uv * vec2(2, -2) + vec2(-1, 1), 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
