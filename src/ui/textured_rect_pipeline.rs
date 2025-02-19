use image::DynamicImage;
use std::collections::{BTreeMap, HashMap};

use cecs::prelude::*;
use glam::Vec2;
use wgpu::{include_wgsl, util::DeviceExt};

use crate::{
    assets::{AssetId, AssetsPlugin, Handle, WeakHandle},
    renderer::texture::{self, Texture},
    GameWorld, Plugin, Stage,
};

use crate::renderer::{
    Extract, ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput,
    RenderCommandPlugin, RenderPass,
};

use super::UiScissor;

pub struct UiTexture {
    pub image: DynamicImage,
    /// Size of the entire image
    pub size: Vec2,
}

impl UiTexture {
    pub fn from_image(image: DynamicImage) -> Self {
        Self {
            size: Vec2::new(image.width() as f32, image.height() as f32),
            image,
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct TextureInstance {
    pub index: u32,
    pub flip: bool,
}

pub fn add_missing_textures(
    mut pipeline: ResMut<TexturePipeline>,
    renderer: Res<GraphicsState>,
    game_world: Res<GameWorld>,
) {
    game_world
        .world()
        .run_view_system(|sheets: Res<crate::assets::Assets<UiTexture>>| {
            for (id, sheet) in sheets.iter() {
                if !pipeline.textures.contains_key(&id) {
                    pipeline.add_sheet(id, sheet, &renderer);
                }
            }
        });
}

fn unload_sheets(
    mut handles: ResMut<RenderTexturesheetHandles>,
    mut pipeline: ResMut<TexturePipeline>,
    mut instances: ResMut<TexturePipelineInstances>,
) {
    let unloaded = handles
        .0
        .iter()
        .filter(|(_, h)| h.upgrade().is_none())
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for id in unloaded {
        pipeline.unload_sheet(id);
        instances.0.remove(&id);
        handles.0.remove(&id);
    }
}

fn compute_text_rect_instances(
    mut q: Query<(
        &crate::transform::GlobalTransform,
        &TextureInstance,
        &mut TextureInstanceRaw,
    )>,
) {
    for (tr, i, instance) in q.iter_mut() {
        let pos = tr.0.pos;
        let scale = tr.0.scale;
        *instance = TextureInstanceRaw {
            index: i.index,
            pos_scale: [pos.x, pos.y, pos.z, scale.x],
            flip: i.flip as u32,
        };
    }
}

/// XY is the center, WH are half-extents
#[derive(Debug, Default, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct DrawRectInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub layer: f32,
}

impl DrawRectInstance {
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: size_of::<[u32; 4]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

#[derive(Default)]
struct TexturePipelineInstances(BTreeMap<AssetId, Vec<TextureInstanceRaw>>);

fn clear_pipeline_instances(mut instances: ResMut<TexturePipelineInstances>) {
    for i in instances.0.values_mut() {
        i.clear();
    }
}

impl Extract for TextureInstanceRaw {
    type QueryItem = (&'static Handle<UiTexture>, &'static TextureInstanceRaw);

    type Filter = ();

    type Out = (WeakHandle<UiTexture>, TextureInstanceRaw);

    fn extract<'a>(
        (handle, instance): <Self::QueryItem as cecs::query::QueryFragment>::Item<'a>,
    ) -> Option<Self::Out> {
        Some((handle.downgrade(), *instance))
    }
}

fn update_sprite_pipelines(
    renderer: Res<GraphicsState>,
    q: Query<(&WeakHandle<UiTexture>, &TextureInstanceRaw)>,
    mut pipeline: ResMut<TexturePipeline>,
    mut instances: ResMut<TexturePipelineInstances>,
) {
    for (handle, raw) in q.iter() {
        instances.0.entry(handle.id()).or_default().push(*raw);
    }

    for (id, cpu) in instances.0.iter() {
        let Some(sprite_rendering_data) = pipeline.textures.get_mut(&id) else {
            continue;
        };

        let instance_data_bytes = bytemuck::cast_slice::<_, u8>(&cpu);
        let size = instance_data_bytes.len() as u64;
        if sprite_rendering_data.instance_gpu.size() < size {
            // resize the buffer
            sprite_rendering_data.instance_gpu =
                renderer.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Texture Instance Buffer - {}", id)),
                    size: size * 2,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        }
        renderer.queue().write_buffer(
            &sprite_rendering_data.instance_gpu,
            0,
            bytemuck::cast_slice(&cpu),
        );
        sprite_rendering_data.count = cpu.len();
    }
}

#[derive(Default)]
struct RenderTexturesheetHandles(pub HashMap<AssetId, WeakHandle<UiTexture>>);

// per texture
pub struct TextureRenderingData {
    pub texture_bind_group: wgpu::BindGroup,
    pub texture: Texture,
}

pub struct TexturePipeline {
    textures: HashMap<AssetId, TextureRenderingData>,
    // shared
    render_pipeline: wgpu::RenderPipeline,

    instances: HashMap<UiScissor, Vec<TexturePipelineInstances>>,
}

impl TexturePipeline {
    pub fn unload_sheet(&mut self, id: AssetId) {
        self.textures.remove(&id);
    }

    pub fn add_sheet(&mut self, id: AssetId, sheet: &UiTexture, renderer: &GraphicsState) {
        let texture = Texture::from_image(renderer.device(), renderer.queue(), &sheet.image, None)
            .expect("Failed to create texture");

        let (_, texture_bind_group) = texture_to_bindings(renderer.device(), &texture);

        self.textures.insert(
            id,
            TextureRenderingData {
                count: 0,
                instance_gpu: renderer.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Texture Instance Buffer"),
                    mapped_at_creation: false,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    size: 0,
                }),
                texture_bind_group,
                texture,
            },
        );
    }

    pub fn new(renderer: &GraphicsState) -> Self {
        let texture_layout: wgpu::BindGroupLayout =
            renderer
                .device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Texture Sheet Uniform Layout"),
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
            .device()
            .create_shader_module(include_wgsl!("textured-rect-shader.wgsl"));

        let texture_bind_group_layout =
            texture_bind_group_layout(renderer.device(), "sprite-texture-layout");

        let render_pipeline_layout =
            renderer
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Texture Render Pipeline Layout"),
                    bind_group_layouts: &[
                        renderer.camera_bind_group_layout(),
                        &texture_bind_group_layout,
                        &texture_layout,
                    ],
                    push_constant_ranges: &[],
                });
        let render_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Texture Render Pipeline"),
                    layout: Some(&render_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[DrawRectInstance::desc(), TextureInstanceRaw::desc()],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: "fs_main",
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: renderer.config().format,
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
                        format: Texture::DEPTH_FORMAT,
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
                    cache: None,
                });

        TexturePipeline {
            textures: Default::default(),
            render_pipeline,
        }
    }
}

struct TextureRenderCommand;

impl<'a> RenderCommand<'a> for TextureRenderCommand {
    type Parameters = (
        Res<'a, crate::renderer::WindowSize>,
        Res<'a, TexturePipeline>,
    );

    fn render<'r>(
        RenderCommandInput {
            render_pass,
            camera,
        }: &'r mut RenderCommandInput<'a>,
        (size, pipeline): &'r Self::Parameters,
    ) {
        render_pass.set_pipeline(&pipeline.render_pipeline);
        for (scissor, instances) in pipeline.instances.iter() {
            let x = scissor.0.min_x.max(0) as u32;
            let y = scissor.0.min_y.max(0) as u32;
            let w = (scissor.0.width() as u32).min(size.width.saturating_sub(x));
            let h = (scissor.0.height() as u32).min(size.height.saturating_sub(y));

            if w == 0 || h == 0 {
                tracing::warn!(?scissor, "Scissor is outside of render target {:?}", **size);
                continue;
            }

            render_pass.set_scissor_rect(x, y, w, h);

            for instances in instances {
                for (id, instances) in instances.0.iter() {
                    let Some(texture) = pipeline.textures.get(id) else {
                        continue;
                    };
                    render_pass.set_bind_group(0, &texture.texture_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, instances.as_slice());

                    render_pass.draw_indexed(0..6, 0, 0..texture.count as u32);
                }
            }
        }
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextureInstanceRaw {
    pos_scale: [f32; 4],
    index: u32,
    /// bool
    flip: u32,
}

impl TextureInstanceRaw {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        const ROW_SIZE: wgpu::BufferAddress = mem::size_of::<[f32; 4]>() as wgpu::BufferAddress;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<TextureInstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: ROW_SIZE,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: ROW_SIZE + 4,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

fn setup(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let sprite_pipeline = TexturePipeline::new(&graphics_state);
    cmd.insert_resource(sprite_pipeline);
}

pub struct TexturedRectRendererPlugin;

impl Plugin for TexturedRectRendererPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(AssetsPlugin::<UiTexture>::default());
        app.add_plugin(ExtractionPlugin::<TextureInstanceRaw>::default());
        app.with_stage(Stage::Update, |s| {
            // putting this system in update means that the last frame's data will be presented
            s.add_system(compute_text_rect_instances);
        });

        app.add_plugin(RenderCommandPlugin::<TextureRenderCommand>::new(
            RenderPass::Ui,
        ));
        app.extact_stage.add_system(add_missing_textures);

        if let Some(ref mut app) = app.render_app {
            app.add_startup_system(setup);
            app.insert_resource(TexturePipelineInstances::default());
            app.insert_resource(RenderTexturesheetHandles::default());
            app.with_stage(Stage::PreUpdate, |s| {
                s.add_system(clear_pipeline_instances);
            });
            app.with_stage(Stage::Update, |s| {
                s.add_system(unload_sheets)
                    .add_system(update_sprite_pipelines);
            });
        }
    }
}

fn texture_bind_group_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                // This should match the filterable field of the
                // corresponding Texture entry above.
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
        label: Some(label),
    })
}

fn texture_to_bindings(
    device: &wgpu::Device,
    texture: &texture::Texture,
) -> (wgpu::BindGroupLayout, wgpu::BindGroup) {
    let texture_bind_group_layout = texture_bind_group_layout(device, "texture_bind_group_layout");
    let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &texture_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&texture.sampler),
            },
        ],
        label: Some("sprite_texture_bind_group"),
    });
    (texture_bind_group_layout, diffuse_bind_group)
}
