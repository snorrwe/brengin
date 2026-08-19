struct Vertex {
    @builtin(vertex_index) vertex_index: u32,
}

struct Instance {
    @location(0) xywh: vec4<f32>,
    @location(1) color: u32,
    @location(2) layer: f32,
    @location(3) uv: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0)
var texture: texture_2d<f32>;
@group(0) @binding(1)
var texture_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c <= vec3<f32>(0.04045);
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, cutoff);
}

@vertex
fn vs_main(
    model: Vertex,
    instance: Instance,
) -> VertexOutput {
    var out: VertexOutput;
    let c = instance.color;
    let xywh = instance.xywh;
    // input colors are sRGB, the surface encodes on write
    let rgb = vec3<f32>(
        f32((c >> 24) & 0xFF) / 255.0,
        f32((c >> 16) & 0xFF) / 255.0,
        f32((c >> 8) & 0xFF) / 255.0,
    );
    out.color = vec4<f32>(srgb_to_linear(rgb), f32(c & 0xFF) / 255.0);

    let u = f32(model.vertex_index & 1);
    var v = f32((model.vertex_index >> 1) & 1);
    let flip = model.vertex_index > 2;
    if flip {
        v = 1.0 - v;
    }

    let uv = vec2<f32>(u, v);
    // text textures are rendered upside down
    let flipped_uv = vec2<f32>(uv.x, 1.0 - uv.y);
    out.uv = mix(instance.uv.xy, instance.uv.zw, flipped_uv);

    var vertex = uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    vertex *= xywh.zw;

    // pos is in 0..1
    // remap to -1..1
    let pos = xywh.xy * 2.0 - 1.0;

    out.clip_position = vec4<f32>(pos + vertex, instance.layer, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = in.color;
    let alpha = textureSample(texture, texture_sampler, in.uv).a * color.a;
    if alpha < 0.001 {
        discard;
    }
    return vec4<f32>(color.rgb, alpha);
}
