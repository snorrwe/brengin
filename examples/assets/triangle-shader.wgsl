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
    var x = f32(vertex.vertex_index % 2);
    var y = f32(vertex.vertex_index == 2);
    out.uv.x = x;
    out.uv.y = y;
    out.clip_position = vec4<f32>(x / 2.0 - 0.5, y / 2.0 - 0.5, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.uv, 0.0, 1.0);
}
