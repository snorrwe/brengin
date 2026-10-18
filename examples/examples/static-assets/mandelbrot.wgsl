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

fn mandelbrot(c: vec2<f32>) -> f32 {
    let B: f32 = 256.0;
    var l = 0.0;
    var z = vec2<f32>(0);
    for (var i = 0; i < 512; i++) {
        z = vec2<f32>(z.x * z.x - z.y * z.y, 2 * z.x * z.y) + c;
        if dot(z, z) > (B * B) {break;}
        l += 1.0;
    }
    if l > 511.0 {
        return 0.0;
    }

    return l - log2(log2(dot(z, z))) + 4.0;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let l = mandelbrot(in.uv * 2 - vec2(1.5, 1));
    let color = 1.0 - 0.5 + 0.5 * cos(0.3 + l * 0.15 + vec3(0.6, 0.6, 0.0));
    return vec4<f32>(color, 1.0);
}
