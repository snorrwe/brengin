use std::collections::HashMap;
use std::mem::size_of;

use crate::assets::{AssetId, Assets, WeakHandle};
use crate::renderer::texture::Texture;
use crate::renderer::{
    texture, ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput,
    RenderCommandPlugin, RenderPass,
};
use crate::wgpu::include_wgsl;
use crate::GameWorld;
use cecs::prelude::*;
use tracing::debug;
use wgpu::util::{DeviceExt as _, RenderEncoder};

use crate::{renderer::Extract, Plugin};

#[derive(Default, Clone, Debug)]
pub struct RectRequests(pub Vec<DrawRect>);

struct RectInstanceBuffer {
    buffer: wgpu::Buffer,
    n: usize,
}

impl Extract for RectRequests {
    type QueryItem = &'static Self;

    type Filter = ();

    type Out = (Self,);

    fn extract<'a>(
        it: <Self::QueryItem as cecs::query::QueryFragment>::Item<'a>,
    ) -> Option<Self::Out> {
        Some((it.clone(),))
    }
}

/// XY are top-left corner, WH are full-extents
#[derive(Debug, Default, Clone, Copy)]
pub struct DrawRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub layer: u16,
    pub color: u32,
}

/// XY is the center, WH are half-extents
#[derive(Debug, Default, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct DrawRectInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: u32,
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
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: (size_of::<[u32; 4]>() + size_of::<u32>()) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

struct RectPipeline {
    color_rect_pipeline: wgpu::RenderPipeline,
    ui_rect_layout: wgpu::BindGroupLayout,
    textures: HashMap<AssetId, UiTextureRenderingData>,
}

pub struct UiTextureRenderingData {
    pub texture_bind_group: wgpu::BindGroup,
    pub texture: Texture,
}

#[derive(Default)]
struct UiTextureReferences(pub HashMap<AssetId, WeakHandle<super::ShapingResult>>);

fn gc_text_textures(
    mut texturerefs: ResMut<UiTextureReferences>,
    mut pipeline: ResMut<RectPipeline>,
) {
    texturerefs.0.retain(|id, handle| {
        if handle.upgrade().is_none() {
            debug!(id, "Collecting expired text texture");
            pipeline.textures.remove(id);
            return false;
        }
        true
    });
}

fn extract_shaping_results(
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<RectPipeline>,
    mut refs: ResMut<UiTextureReferences>,
    game_world: Res<GameWorld>,
) {
    game_world.world().run_view_system(
        |cache: Res<super::TextTextureCache>,
         shaping_results: Res<Assets<super::ShapingResult>>| {
            for handle in cache.0.values() {
                let res = shaping_results.get(handle);
                let id = handle.id();
                if !refs.0.contains_key(&id) {
                    let texture = Texture::from_rgba8(
                        renderer.device(),
                        renderer.queue(),
                        res.texture.pixmap.data(),
                        (res.texture.width(), res.texture.height()),
                        None,
                    )
                    .expect("Failed to create text texture");
                    let texture_bind_group = texture_to_bindings(renderer.device(), &texture);

                    refs.0.insert(id, handle.downgrade());
                    let rendering_data = UiTextureRenderingData {
                        texture_bind_group,
                        texture,
                    };

                    pipeline.textures.insert(id, rendering_data);
                };
            }
        },
    );
}

impl RectPipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let ui_rect_layout: wgpu::BindGroupLayout =
            renderer
                .device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Ui Color Rect Uniform Layout"),
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
            .create_shader_module(include_wgsl!("ui-rect.wgsl"));

        let texture_bind_group_layout =
            texture_bind_group_layout(renderer.device(), "ui-texture-layout");

        let color_rect_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Ui Color Rect Render Pipeline"),
                    layout: Some(&renderer.device().create_pipeline_layout(
                        &wgpu::PipelineLayoutDescriptor {
                            label: Some("Ui Color Rect Render Pipeline Layout"),
                            bind_group_layouts: &[
                                // TODO:
                                // &texture_bind_group_layout,
                            ],
                            push_constant_ranges: &[],
                        },
                    )),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[DrawRectInstance::desc()],
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
                        polygon_mode: wgpu::PolygonMode::Fill,
                        unclipped_depth: false,
                        conservative: false,
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: texture::Texture::DEPTH_FORMAT,
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

        RectPipeline {
            ui_rect_layout,
            color_rect_pipeline,
            textures: Default::default(),
        }
    }
}

struct RectRenderCommand;

impl<'a> RenderCommand<'a> for RectRenderCommand {
    type Parameters = (
        Query<'a, &'static RectInstanceBuffer>,
        Res<'a, RectPipeline>,
    );

    fn render<'r>(input: &'r mut RenderCommandInput<'a>, (rects, pipeline): &'r Self::Parameters) {
        input
            .render_pass
            .set_pipeline(&pipeline.color_rect_pipeline);
        for requests in rects.iter() {
            input
                .render_pass
                .set_vertex_buffer(0, requests.buffer.slice(..));
            input.render_pass.draw(0..6, 0..requests.n as u32);
        }
    }
}

fn setup_renderer(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pipeline = RectPipeline::new(&graphics_state);
    cmd.insert_resource(pipeline);
}

fn update_instances(
    q: Query<(EntityId, &RectRequests, Option<&RectInstanceBuffer>)>,
    renderer: Res<GraphicsState>,
    mut cmd: Commands,
) {
    // TODO: retain buffer
    let mut buff = Vec::new();
    let w = renderer.size().width as f32;
    let h = renderer.size().height as f32;
    for (id, rects, buffer) in q.iter() {
        buff.clear();
        buff.reserve(rects.0.len());
        buff.extend(rects.0.iter().map(|rect| {
            let ww = rect.w as f32 * 0.5;
            let hh = rect.h as f32 * 0.5;
            // flip y
            let y = h - rect.y as f32;
            // switch order of layers, lower layers are in the front
            // remap to 0..1
            let layer = (0xFFFF - rect.layer) as f32 / (0xFFFF as f32);
            DrawRectInstance {
                x: (rect.x as f32 + ww) / w,
                y: (y - hh) / h,
                // w: ww / w,
                w: rect.w as f32 / w,
                h: rect.h as f32 / h,
                layer,
                color: rect.color,
            }
        }));

        match buffer {
            Some(buffer) if rects.0.len() <= buffer.n => {
                renderer.queue().write_buffer(
                    &buffer.buffer,
                    0,
                    bytemuck::cast_slice(buff.as_slice()),
                );
            }
            _ => {
                let buffer =
                    renderer
                        .device()
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some(&format!("Rect Instance Buffer {}", id)),
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            contents: bytemuck::cast_slice(buff.as_slice()),
                        });
                cmd.entity(id).insert(RectInstanceBuffer {
                    buffer,
                    n: rects.0.len(),
                });
            }
        }
    }
}

pub struct UiCorePlugin;

impl Plugin for UiCorePlugin {
    fn build(self, app: &mut crate::App) {
        app.insert_resource(RectRequests::default());
        app.add_plugin(ExtractionPlugin::<RectRequests>::default());

        app.add_plugin(RenderCommandPlugin::<RectRenderCommand>::new(
            RenderPass::Ui,
        ));
        app.extact_stage.add_system(extract_shaping_results);
        if let Some(ref mut renderer) = app.render_app {
            renderer.add_startup_system(setup_renderer);
            renderer.with_stage(crate::Stage::Update, |s| {
                s.add_system(update_instances);
            });
            renderer.insert_resource(UiTextureReferences::default());
            renderer.with_stage(crate::Stage::PostUpdate, |s| {
                s.add_system(gc_text_textures);
            });
        }
    }
}

fn texture_to_bindings(device: &wgpu::Device, texture: &texture::Texture) -> wgpu::BindGroup {
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
    diffuse_bind_group
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
