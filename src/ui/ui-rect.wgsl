struct Vertex {
}

struct Instance {
    @location(2) xywh: vec4<u32>,
    @location(3) color: u32,
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
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(texture, texture_sampler, in.uv);
    return color;
}
