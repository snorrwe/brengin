@group(0) @binding(0)
var texture: texture_2d<f32>;
@group(0) @binding(1)
var texture_sampler: sampler;

struct Vertex {
    @builtin(vertex_index) vertex_index: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let u = f32((vertex.vertex_index << 1) & 2);
    let v = f32(vertex.vertex_index & 2);
    let uv = vec2<f32>(u, v);
    out.uv = uv;
    out.clip_position = vec4<f32>(uv * vec2<f32>(2, -2) + vec2<f32>(-1, 1), 0, 1);
    return out;
}


@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(texture, texture_sampler, in.uv);
    return vec4<f32>(color.rgb, 1.0);
}
