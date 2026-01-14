struct Vertex {
    @builtin(vertex_index) vertex_index: u32,
}

struct Instance {
    @location(0) xywh: vec4<f32>,
    @location(1) color: u32,
    @location(2) layer: f32,
    // normalized radius
    @location(3) radius: vec2<f32>,
    @location(4) outline_color: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) radius: vec2<f32>,
    @location(3) outline_color: vec4<f32>,
}

fn parse_color(c: u32) -> vec4<f32> {
    return vec4<f32>(
            f32((c >> 24) & 0xFF) / 255.0,
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
    let c = instance.color;
    let xywh = instance.xywh;
    out.color = parse_color(instance.color);
    out.outline_color = parse_color(instance.outline_color);

    let u = f32(model.vertex_index & 1);
    var v = f32((model.vertex_index >> 1) & 1);
    let flip = model.vertex_index > 2;
    if flip {
        v = 1.0 - v;
    }

    var vertex = vec2<f32>(u, v) * vec2(2, -2) + vec2(-1, 1);
    vertex *= xywh.zw;

    // pos is in 0..1
    // remap to -1..1
    let pos = xywh.xy * 2 - 1;

    out.clip_position = vec4<f32>(pos + vertex, instance.layer, 1.0);
    out.radius = instance.radius;
    out.uv = vec2<f32>(u,v);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let x = min(uv.x, 1.0 - uv.x);
    let y = min(uv.y, 1.0 - uv.y);
    if length(in.outline_color) != 0 && (x < in.radius.x || y < in.radius.y) {
        return in.outline_color;
    }
    if length(in.color) == 0 {
        discard;
    }
    return in.color;
}
