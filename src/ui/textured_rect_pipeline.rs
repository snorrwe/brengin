use std::collections::HashMap;
use std::mem::size_of;

use crate::assets::{AssetId, Assets, Handle, WeakHandle};
use crate::renderer::texture::Texture;
use crate::renderer::{
    texture, ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput,
    RenderCommandPlugin, RenderPass,
};
use crate::wgpu::include_wgsl;
use crate::GameWorld;
use cecs::prelude::*;
use image::DynamicImage;

use crate::{renderer::Extract, Plugin};

use super::UiScissor;

#[derive(Default, Clone)]
pub struct TextureRectRequests(pub Vec<DrawTextureRect>);

impl Extract for TextureRectRequests {
    type QueryItem = (&'static Self, &'static UiScissor);

    type Filter = ();

    type Out = (Self, UiScissor);

    fn extract<'a>(
        (it, sc): <Self::QueryItem as cecs::query::QueryFragment>::Item<'a>,
    ) -> Option<Self::Out> {
        Some((it.clone(), *sc))
    }
}

/// XY are top-left corner, WH are full-extents
#[derive(Default, Clone)]
pub struct DrawTextureRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub layer: u16,
    pub image: Handle<DynamicImage>,
    pub scissor: u32,
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

struct UiTexturePipeline {
    pipeline: wgpu::RenderPipeline,
    textures: HashMap<AssetId, UiTextureRenderingData>,
    instances: HashMap<(UiScissor, AssetId), UiTextureRenderingInstances>,
}

pub struct UiTextureRenderingInstances {
    pub count: usize,
    pub instance_gpu: wgpu::Buffer,
}

pub struct UiTextureRenderingData {
    pub texture_bind_group: wgpu::BindGroup,
    pub texture: Texture,
}

#[derive(Default)]
struct UiTextureReferences(pub HashMap<AssetId, WeakHandle<DynamicImage>>);

fn gc_text_textures(
    mut texturerefs: ResMut<UiTextureReferences>,
    mut pipeline: ResMut<UiTexturePipeline>,
) {
    texturerefs.0.retain(|id, handle| {
        if handle.upgrade().is_none() {
            #[cfg(feature = "tracing")]
            tracing::debug!(id, "Collecting expired text texture");
            pipeline.textures.remove(id);
            return false;
        }
        true
    });
}

// TODO: extract textures from ui
fn extract_textures(
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<UiTexturePipeline>,
    mut refs: ResMut<UiTextureReferences>,
    game_world: Res<GameWorld>,
) {
    game_world.world().run_view_system(
        |requests: Query<&TextureRectRequests>, images: Res<Assets<DynamicImage>>| {
            for r in requests.iter() {
                for handle in r.0.iter().map(|r| &r.image) {
                    let res = images.get(handle);
                    let id = handle.id();
                    if refs.0.contains_key(&id) {
                        continue;
                    }
                    let texture =
                        Texture::from_image(renderer.device(), renderer.queue(), res, None)
                            .expect("Failed to create text texture");
                    let texture_bind_group = texture_to_bindings(renderer.device(), &texture);

                    refs.0.insert(id, handle.downgrade());
                    let rendering_data = UiTextureRenderingData {
                        texture_bind_group,
                        texture,
                    };

                    pipeline.textures.insert(id, rendering_data);
                }
            }
        },
    );
}

impl UiTexturePipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let shader = renderer
            .device()
            .create_shader_module(include_wgsl!("textured-rect-shader.wgsl"));

        let texture_bind_group_layout =
            texture_bind_group_layout(renderer.device(), "ui-text-layout");

        let color_rect_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Ui Texture Rect Render Pipeline"),
                    layout: Some(&renderer.device().create_pipeline_layout(
                        &wgpu::PipelineLayoutDescriptor {
                            label: Some("Ui Texture Rect Render Pipeline Layout"),
                            bind_group_layouts: &[&texture_bind_group_layout],
                            ..Default::default()
                        },
                    )),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[DrawRectInstance::desc()],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
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
                        alpha_to_coverage_enabled: false,
                    },
                    multiview_mask: None,
                    cache: None,
                });

        UiTexturePipeline {
            pipeline: color_rect_pipeline,
            textures: Default::default(),
            instances: Default::default(),
        }
    }
}

struct RectRenderCommand;

impl<'a> RenderCommand<'a> for RectRenderCommand {
    type Parameters = (
        Res<'a, crate::renderer::WindowSize>,
        Res<'a, UiTexturePipeline>,
    );

    fn render<'r>(
        input: &'r mut RenderCommandInput<'a, 'r>,
        (size, pipeline): &'r Self::Parameters,
    ) {
        input.render_pass.set_pipeline(&pipeline.pipeline);

        for ((scissor, texture_id), requests) in pipeline.instances.iter() {
            let x = scissor.0.min_x.max(0) as u32;
            let y = scissor.0.min_y.max(0) as u32;
            let w = (scissor.0.width() as u32).min(size.width.saturating_sub(x));
            let h = (scissor.0.height() as u32).min(size.height.saturating_sub(y));

            if w == 0 || h == 0 {
                #[cfg(feature = "tracing")]
                tracing::warn!(?scissor, "Scissor is outside of render target {:?}", **size);
                continue;
            }

            input.render_pass.set_scissor_rect(x, y, w, h);
            let Some(texture) = pipeline.textures.get(texture_id) else {
                continue;
            };
            input
                .render_pass
                .set_vertex_buffer(0, requests.instance_gpu.slice(..));
            input
                .render_pass
                .set_bind_group(0, &texture.texture_bind_group, &[]);
            input.render_pass.draw(0..6, 0..requests.count as u32);
        }
    }
}

fn setup_renderer(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pipeline = UiTexturePipeline::new(&graphics_state);
    cmd.insert_resource(pipeline);
}

fn update_instances(
    q: Query<(&TextureRectRequests, &UiScissor)>,
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<UiTexturePipeline>,
) {
    // TODO: retain buffer
    let w = renderer.size().x as f32;
    let h = renderer.size().y as f32;
    let mut instances = HashMap::<(AssetId, UiScissor), Vec<DrawRectInstance>>::default();
    for (rects, scissor) in q.iter() {
        for rect in rects.0.iter() {
            let half_w = rect.w as f32 * 0.5;
            let half_h = rect.h as f32 * 0.5;
            // flip y
            let y = h - rect.y as f32;
            // switch order of layers, lower layers are in the front
            // remap to 0..1
            let layer = (0xFFFF - rect.layer) as f32 / (0xFFFF as f32);
            let instance = DrawRectInstance {
                x: (rect.x as f32 + half_w) / w,
                y: (y - half_h) / h,
                // w: ww / w,
                w: rect.w as f32 / w,
                h: rect.h as f32 / h,
                layer,
            };
            instances
                .entry((rect.image.id(), *scissor))
                .or_default()
                .push(instance);
        }
    }

    // FIXME: retain buffers or do a smarter gc
    pipeline.instances.clear();
    for ((id, scissor), cpu) in instances.iter() {
        let rendering_data = pipeline
            .instances
            .entry((*scissor, *id))
            .or_insert_with(|| UiTextureRenderingInstances {
                count: 0,
                instance_gpu: renderer.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Texture Instance Buffer - {:?} {}", scissor, id)),
                    mapped_at_creation: false,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    size: 0,
                }),
            });

        let instance_data_bytes = bytemuck::cast_slice::<_, u8>(cpu.as_slice());
        let size = instance_data_bytes.len() as u64;
        if rendering_data.instance_gpu.size() < size {
            // resize the buffer
            rendering_data.instance_gpu =
                renderer.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!(
                        "UI Texture Instance Buffer - {:?} {}",
                        scissor, id
                    )),
                    size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        }
        renderer
            .queue()
            .write_buffer(&rendering_data.instance_gpu, 0, bytemuck::cast_slice(&cpu));
        rendering_data.count = cpu.len();
    }
}

pub struct UiTextureRectPlugin;

impl Plugin for UiTextureRectPlugin {
    fn build(self, app: &mut crate::App) {
        app.insert_resource(TextureRectRequests::default());
        app.add_plugin(ExtractionPlugin::<TextureRectRequests>::default());

        app.add_plugin(RenderCommandPlugin::<RectRenderCommand>::new(
            RenderPass::Ui,
        ));
        app.extract_stage.add_system(extract_textures);
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
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        label: Some("text_texture_bind_group"),
    });
    bind_group
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
