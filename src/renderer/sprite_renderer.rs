use std::collections::{BTreeMap, HashMap};

use cecs::prelude::*;
use glam::Vec2;
use wgpu::{include_wgsl, util::DeviceExt};

use crate::{
    assets::{AssetId, Assets, Handle},
    camera::ViewFrustum,
    transform::GlobalTransform,
    Plugin,
};

use super::{texture::Texture, GraphicsState, Vertex};

pub fn sprite_sheet_bundle(
    handle: Handle<SpriteSheet>,
    instance: impl Into<Option<SpriteInstance>>,
) -> impl Bundle {
    (
        instance.into().unwrap_or(SpriteInstance {
            index: 0,
            flip: false,
        }),
        Visible,
        handle,
        SpriteInstanceRaw::default(),
    )
}

struct CullSize(pub f32);
struct Visible;

fn signed_dist_plane_point(plane: &glam::Vec4, pos: glam::Vec3) -> f32 {
    plane.dot(pos.extend(1.0))
}

fn is_visible(pos: glam::Vec3, planes: &[glam::Vec4], radius: f32) -> bool {
    for plane in planes {
        let d = signed_dist_plane_point(plane, pos);
        if d < -radius {
            return false;
        }
    }
    true
}

fn update_visible(
    mut cmd: Commands,
    cameras: Query<&ViewFrustum>,
    visible: Query<(EntityId, &CullSize, &GlobalTransform), With<Visible>>,
) {
    for fr in cameras.iter() {
        for (id, size, tr) in visible.iter() {
            if !is_visible(tr.0.pos, &fr.planes, size.0) {
                cmd.entity(id).remove::<Visible>();
            }
        }
    }
}

fn update_invisible(
    mut cmd: Commands,
    cameras: Query<&ViewFrustum>,
    invisible: Query<(EntityId, &CullSize, &GlobalTransform), WithOut<Visible>>,
) {
    for fr in cameras.iter() {
        for (id, size, tr) in invisible.iter() {
            if is_visible(tr.0.pos, &fr.planes, size.0) {
                cmd.entity(id).insert(Visible);
            }
        }
    }
}

fn insert_missing_cull(
    q: Query<(EntityId, &Handle<SpriteSheet>), WithOut<CullSize>>,
    assets: Res<Assets<SpriteSheet>>,
    mut cmd: Commands,
) {
    for (id, handle) in q.iter() {
        let sheet = assets.get(handle);
        cmd.entity(id)
            .insert(CullSize(sheet.box_size.x.max(sheet.box_size.y)));
    }
}

pub struct SpriteSheet {
    /// Padding applied to the box
    pub padding: Vec2,
    /// Size of the entire box
    pub box_size: Vec2,
    /// Number of boxes in a row
    pub num_cols: u32,
    pub texture: Texture,
    /// Size of the entire sheet
    pub size: Vec2,
}

impl SpriteSheet {
    pub fn from_texture(padding: Vec2, box_size: Vec2, num_cols: u32, texture: Texture) -> Self {
        Self {
            padding,
            box_size,
            num_cols,
            size: Vec2::new(texture.size.0 as f32, texture.size.1 as f32),
            texture,
        }
    }

    fn extract(&self) -> SpriteSheetGpu {
        SpriteSheetGpu {
            padding: self.padding.to_array(),
            box_size: self.box_size.to_array(),
            num_cols: self.num_cols,
            size: self.size.to_array(),
            _pad: 0xDEADBEEF,
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct SpriteInstance {
    pub index: u32,
    pub flip: bool,
}

pub fn add_missing_sheets(
    mut pipeline: ResMut<SpritePipeline>,
    sheets: Res<crate::assets::Assets<SpriteSheet>>,
    renderer: Res<GraphicsState>,
) {
    for (id, sheet) in sheets.iter() {
        if !pipeline.sheets.contains_key(&id) {
            pipeline.add_sheet(id, sheet, &renderer);
        }
    }
}

fn unload_sheets(
    mut pipeline: ResMut<SpritePipeline>,
    sheets: Res<crate::assets::Assets<SpriteSheet>>,
    mut instances: ResMut<SpritePipelineInstances>,
) {
    let unloaded = pipeline
        .sheets
        .keys()
        .filter(|id| !sheets.contains(**id))
        .copied()
        .collect::<Vec<_>>();
    for id in unloaded {
        pipeline.unload_sheet(id);
        instances.0.remove(&id);
    }
}

fn compute_sprite_instances(
    mut q: Query<
        (
            &crate::transform::GlobalTransform,
            &SpriteInstance,
            &mut SpriteInstanceRaw,
        ),
        With<Visible>,
    >,
) {
    q.par_for_each_mut(|(tr, i, instance)| {
        *instance = SpriteInstanceRaw {
            index: i.index,
            model: tr.0.compute_matrix().to_cols_array_2d(),
            flip: i.flip as u32,
        };
    });
}

#[derive(Default)]
struct SpritePipelineInstances(BTreeMap<u64, Vec<SpriteInstanceRaw>>);

fn clear_pipeline_instances(mut instances: ResMut<SpritePipelineInstances>) {
    for i in instances.0.values_mut() {
        i.clear();
    }
}

fn update_sprite_pipelines(
    renderer: Res<GraphicsState>,
    q: Query<(&Handle<SpriteSheet>, &SpriteInstanceRaw), With<Visible>>,
    mut pipeline: ResMut<SpritePipeline>,
    mut instances: ResMut<SpritePipelineInstances>,
) {
    for (handle, raw) in q.iter() {
        instances.0.entry(handle.id()).or_default().push(*raw);
    }

    for (id, cpu) in instances.0.iter() {
        let sprite_rendering_data = pipeline.sheets.get_mut(&id).unwrap();

        let instance_data_bytes = bytemuck::cast_slice::<_, u8>(&cpu);
        let size = instance_data_bytes.len() as u64;
        if sprite_rendering_data.instance_gpu.size() < size {
            // resize the buffer
            sprite_rendering_data.instance_gpu =
                renderer.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Sprite Instance Buffer - {}", id)),
                    size: size * 2,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        }
        renderer.queue.write_buffer(
            &sprite_rendering_data.instance_gpu,
            0,
            bytemuck::cast_slice(&cpu),
        );
        sprite_rendering_data.count = cpu.len();
    }
}

pub struct SpriteRenderingData {
    // per spritesheet
    count: usize,
    instance_gpu: wgpu::Buffer,
    spritesheet_gpu: wgpu::BindGroup,
    spritesheet_bind_group: wgpu::BindGroup,
}

pub struct SpritePipeline {
    sheets: HashMap<AssetId, SpriteRenderingData>,
    // shared
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    sprite_sheet_layout: wgpu::BindGroupLayout,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteSheetGpu {
    pub padding: [f32; 2],
    pub box_size: [f32; 2],
    pub size: [f32; 2],
    pub num_cols: u32,
    pub _pad: u32,
}

impl SpritePipeline {
    pub fn unload_sheet(&mut self, id: AssetId) {
        self.sheets.remove(&id);
    }

    pub fn add_sheet(&mut self, id: AssetId, sheet: &SpriteSheet, renderer: &GraphicsState) {
        let (_, spritesheet_bind_group) =
            super::texture_to_bindings(&renderer.device, &sheet.texture);
        let sheet_gpu = sheet.extract();

        let spritesheet_buffer =
            renderer
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("SpriteSheet Instance Buffer {}", id)),
                    usage: wgpu::BufferUsages::UNIFORM,
                    contents: bytemuck::cast_slice(&[sheet_gpu]),
                });

        let spritesheet_gpu = renderer
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.sprite_sheet_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: spritesheet_buffer.as_entire_binding(),
                }],
                label: Some(&format!("spritesheet_bind_group {}", id)),
            });

        self.sheets.insert(
            id,
            SpriteRenderingData {
                count: 0,
                instance_gpu: renderer.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Sprite Instance Buffer"),
                    mapped_at_creation: false,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    size: 0,
                }),
                spritesheet_gpu,
                spritesheet_bind_group,
            },
        );
    }

    pub fn new(renderer: &GraphicsState) -> Self {
        let sprite_sheet_layout: wgpu::BindGroupLayout =
            renderer
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Sprite Sheet Uniform Layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::all(),
                        count: None,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                    }],
                });
        let shader = renderer
            .device
            .create_shader_module(include_wgsl!("sprite-shader.wgsl"));

        let texture_bind_group_layout =
            super::texture_bind_group_layout(&renderer.device, "sprite-texture-layout");

        let render_pipeline_layout =
            renderer
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Sprite Render Pipeline Layout"),
                    bind_group_layouts: &[
                        &renderer.camera_bind_group_layout,
                        &texture_bind_group_layout,
                        &sprite_sheet_layout,
                    ],
                    push_constant_ranges: &[],
                });
        let render_pipeline =
            renderer
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Sprite Render Pipeline"),
                    layout: Some(&render_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[Vertex::desc(), SpriteInstanceRaw::desc()],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: "fs_main",
                        targets: &[Some(wgpu::ColorTargetState {
                            format: renderer.config.format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
                        polygon_mode: wgpu::PolygonMode::Fill,
                        // Requires Features::DEPTH_CLIP_CONTROL
                        unclipped_depth: false,
                        // Requires Features::CONSERVATIVE_RASTERIZATION
                        conservative: false,
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: super::texture::Texture::DEPTH_FORMAT,
                        depth_write_enabled: true,
                        depth_compare: wgpu::CompareFunction::Less,
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: 1,
                        mask: !0,
                        alpha_to_coverage_enabled: true,
                    },
                    multiview: None,
                });

        let vertex_buffer = renderer
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sprite Vertex Buffer"),
                contents: bytemuck::cast_slice(VERTICES),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let index_buffer = renderer
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sprite Index Buffer"),
                contents: bytemuck::cast_slice(INDICES),
                usage: wgpu::BufferUsages::INDEX,
            });
        let num_indices = INDICES.len() as u32;

        SpritePipeline {
            sheets: Default::default(),
            sprite_sheet_layout,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            num_indices,
        }
    }

    pub fn render<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        camera: &'a wgpu::BindGroup,
    ) {
        render_pass.set_pipeline(&self.render_pipeline);
        for (_, sheet) in self.sheets.iter() {
            render_pass.set_bind_group(0, camera, &[]);
            render_pass.set_bind_group(1, &sheet.spritesheet_bind_group, &[]);
            render_pass.set_bind_group(2, &sheet.spritesheet_gpu, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, sheet.instance_gpu.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            render_pass.draw_indexed(0..self.num_indices, 0, 0..sheet.count as u32);
        }
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteInstanceRaw {
    index: u32,
    model: [[f32; 4]; 4],
    /// bool
    flip: u32,
}

impl SpriteInstanceRaw {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        const ROW_SIZE: wgpu::BufferAddress = mem::size_of::<[f32; 4]>() as wgpu::BufferAddress;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<SpriteInstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 4,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // A mat4 takes up 4 vertex slots as it is technically 4 vec4s. We need to define a slot
                // for each vec4. We'll have to reassemble the mat4 in
                // the shader.
                wgpu::VertexAttribute {
                    offset: 4 + ROW_SIZE,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 4 + ROW_SIZE * 2,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 4 + ROW_SIZE * 3,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 4 + ROW_SIZE * 4,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

const VERTICES: &[Vertex] = &[
    // A
    Vertex {
        pos: [-0.5, 0.5, 0.0],
        uv: [0.0, 0.0],
    },
    // B
    Vertex {
        pos: [-0.5, -0.5, 0.0],
        uv: [0.0, 1.0],
    },
    // C
    Vertex {
        pos: [0.5, -0.5, 0.0],
        uv: [1.0, 1.0],
    },
    // D
    Vertex {
        pos: [0.5, 0.5, 0.0],
        uv: [1.0, 0.0],
    },
];

const INDICES: &[u16] = &[3, 2, 1, 3, 1, 0];

pub struct SpriteRendererPlugin;

impl Plugin for SpriteRendererPlugin {
    fn build(self, app: &mut crate::App) {
        // putting this system in update means that the last frame's data is presented
        app.stage(crate::Stage::Update)
            .add_system(add_missing_sheets)
            .add_system(compute_sprite_instances)
            .add_system(
                update_sprite_pipelines
                    .after(compute_sprite_instances)
                    .after(add_missing_sheets),
            )
            .add_system(unload_sheets)
            .add_system(insert_missing_cull)
            .add_system(update_visible)
            .add_system(update_invisible);
        app.stage(crate::Stage::PreUpdate)
            .add_system(clear_pipeline_instances);
        app.insert_resource(SpritePipelineInstances::default())
    }
}
