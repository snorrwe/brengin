struct Camera {
    view_proj: mat4x4<f32>,
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
}

@group(2) @binding(0)
var<uniform> sprite_sheet: SpriteSheet;

struct Vertex {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct Instance {
    @location(2) sprite_index: u32,
    @location(3) model_matrix_0: vec4<f32>,
    @location(4) model_matrix_1: vec4<f32>,
    @location(5) model_matrix_2: vec4<f32>,
    @location(6) model_matrix_3: vec4<f32>,
    @location(7) flip: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
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

    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let row: u32 = instance.sprite_index / sprite_sheet.num_cols;
    let col: u32 = instance.sprite_index - sprite_sheet.num_cols * row;

    let offset = sprite_sheet.box_size.xy * vec2<f32>(f32(col), f32(row)) + sprite_sheet.padding;

    var uv = model.uv;
    if (instance.flip != 0u) {
        uv.x = 1.0 - uv.x;
    }
    let box_uv = lerp_vec2(vec2(0.0), sprite_sheet.box_size - sprite_sheet.padding * 2.0, uv) + offset;
    let total_uv = inv_lerp_vec2(vec2(0.0), sprite_sheet.image_size, box_uv);

    let pos = camera.view_proj * model_matrix * vec4<f32>(model.pos, 1.0);
    out.clip_position = pos;
    out.uv = total_uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(texture, texture_sampler, in.uv);
    return color;
}
