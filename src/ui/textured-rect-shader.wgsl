@group(0) @binding(0)
var texture: texture_2d<f32>;
@group(0) @binding(1)
var texture_sampler: sampler;

struct Vertex {
    @builtin(vertex_index) vertex_index: u32,
}

struct Instance {
    @location(0) xywh: vec4<f32>,
    @location(1) layer: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(
    model: Vertex,
    instance: Instance,
) -> VertexOutput {
    var out: VertexOutput;
    let xywh = instance.xywh;

    let u = f32(model.vertex_index & 1);
    var v = f32((model.vertex_index >> 1) & 1);

    let flip = model.vertex_index > 2;
    if flip {
        v = 1.0 - v;
    }

    out.uv = vec2<f32>(u, v);

    var vertex = vec2<f32>(u, v) * vec2(2, -2) + vec2(-1, 1);
    vertex *= xywh.zw;

    // pos is in 0..1
    // remap to -1..1
    let pos = xywh.xy * 2 - 1;

    out.clip_position = vec4<f32>(pos + vertex, instance.layer, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(texture, texture_sampler, in.uv);
    return color;
}
