struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_inv: mat4x4<f32>,
    proj_inv: mat4x4<f32>,
    view_proj_inv: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var texture: texture_2d<f32>;
@group(1) @binding(1)
var texture_sampler: sampler;
@group(2) @binding(0)
var mask: texture_2d<f32>;
@group(2) @binding(1)
var mask_sampler: sampler;

struct SpriteSheet {
    padding: vec2<f32>,
    box_size: vec2<f32>,
    image_size: vec2<f32>,
    num_cols: u32,
}

@group(3) @binding(0)
var<uniform> sprite_sheet: SpriteSheet;

struct Vertex {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct Instance {
    @location(2) pos: vec3<f32>,
    @location(3) scale: vec2<f32>,
    @location(4) uv: vec4<f32>,
    @location(5) mask_uv: vec4<f32>,
    @location(6) color_flip: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) mask_uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) mask_oklab: vec3<f32>,
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

    var uv = model.uv;
    let flip = instance.color_flip & 0xFF;
    if flip != 0u {
        uv.x = 1.0 - uv.x;
    }
    out.uv = mix(instance.uv.xy, instance.uv.zw, uv);
    out.mask_uv = mix(instance.mask_uv.xy, instance.mask_uv.zw, uv);

    // billboarding
    let scale_x = instance.scale.x;
    var pos = vec4<f32>(instance.pos, 1.0);
    let up: vec4<f32> = -camera.view[1];
    let right: vec4<f32> = camera.view[0];

    pos += right * model.pos.x * scale_x;
    pos += up * model.pos.y * instance.scale.y;

    out.clip_position = camera.view_proj * pos;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(texture, texture_sampler, in.uv);
    if color.a < 0.01 {
        discard;
    }

    let mask = textureSample(mask, mask_sampler, in.mask_uv);
    let mask_alpha = mask.r;
    let rgb = mix(color.rgb, in.color, mask_alpha);
    // premultiply in linear space, the pipeline blends with
    // PREMULTIPLIED_ALPHA_BLENDING
    return vec4(rgb * color.a, color.a);
}
