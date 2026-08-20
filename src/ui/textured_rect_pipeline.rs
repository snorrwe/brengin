use std::collections::HashMap;
use std::mem::size_of;

use crate::assets::{AssetId, Assets, Handle, WeakHandle};
use crate::renderer::texture_atlas::{AtlasRect, TextureAtlasRegistry};
use crate::renderer::{
    GraphicsState, RenderCommand, RenderCommandInput, RenderCommandPlugin, RenderPass, texture,
};
use crate::wgpu::include_wgsl;
use cecs::prelude::*;
use image::DynamicImage;

use crate::Plugin;
use crate::renderer::texture_atlas::TextureAtlasPlugin;

use super::UiScissor;

#[derive(Default, Clone)]
pub struct TextureRectRequests(pub Vec<DrawTextureRect>);

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
    pub uv: [f32; 4],
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
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: (2 * size_of::<[u32; 4]>()) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

struct UiTexturePipeline {
    pipeline: wgpu::RenderPipeline,
    /// image_id -> AtlasRect
    image_rects: HashMap<AssetId, AtlasRect>,
    /// atlas_id -> bind group
    /// TODO: invalidate if atlas is released
    atlases: HashMap<AssetId, wgpu::BindGroup>,
    instances: HashMap<UiScissor, Vec<UiTextureRenderingInstances>>,
}

pub struct UiTextureRenderingInstances {
    pub atlas_id: AssetId,
    pub count: usize,
    pub instance_gpu: wgpu::Buffer,
}

#[derive(Default)]
struct UiTextureReferences(pub HashMap<AssetId, WeakHandle<DynamicImage>>);

fn gc_text_textures(
    mut texturerefs: ResMut<UiTextureReferences>,
    mut pipeline: ResMut<UiTexturePipeline>,
    mut textures: TextureAtlasRegistry,
) {
    texturerefs.0.retain(|id, handle| {
        if handle.upgrade().is_none() {
            #[cfg(feature = "tracing")]
            tracing::debug!(id, "Collecting expired text texture");
            if let Some(rect) = pipeline.image_rects.remove(id) {
                textures.deallocate(rect);
            }
            return false;
        }
        true
    });
}

// TODO: extract textures from ui
fn extract_textures(
    mut pipeline: ResMut<UiTexturePipeline>,
    mut refs: ResMut<UiTextureReferences>,
    requests: Query<&TextureRectRequests>,
    images: Res<Assets<DynamicImage>>,
    mut textures: TextureAtlasRegistry,
) {
    for r in requests.iter() {
        for handle in r.0.iter().map(|r| &r.image) {
            let res = images.get(handle);
            let id = handle.id();
            if refs.0.contains_key(&id) {
                continue;
            }

            let Some(rect) = textures.allocate(res.width() as i32, res.height() as i32) else {
                #[cfg(feature = "tracing")]
                tracing::error!(id = handle.id(), "Failed to allocate texture for image");
                continue;
            };
            textures.upload_rgba(&rect, &res.to_rgba8(), res.width(), res.height());
            let atlas_id = rect.atlas_handle().id();
            pipeline.atlases.entry(atlas_id).or_insert_with(|| {
                let (_, texture_bind_group) = textures.get_bind_group(&rect);
                texture_bind_group
            });
            pipeline.image_rects.insert(id, rect);
            refs.0.insert(id, handle.downgrade());
        }
    }
}

impl UiTexturePipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let shader = renderer
            .device()
            .create_shader_module(include_wgsl!("./ui-texture.wgsl"));

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
                            bind_group_layouts: &[Some(&texture_bind_group_layout)],
                            ..Default::default()
                        },
                    )),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[Some(DrawRectInstance::desc())],
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
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
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
            instances: Default::default(),
            atlases: Default::default(),
            image_rects: Default::default(),
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

        for (scissor, requests_list) in pipeline.instances.iter() {
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
            for requests in requests_list.iter() {
                if requests.count == 0 {
                    continue;
                }
                let Some(atlas_bind_group) = pipeline.atlases.get(&requests.atlas_id) else {
                    continue;
                };
                input
                    .render_pass
                    .set_vertex_buffer(0, requests.instance_gpu.slice(..));
                input.render_pass.set_bind_group(0, atlas_bind_group, &[]);
                input.render_pass.draw(0..6, 0..requests.count as u32);
            }
        }
        input
            .render_pass
            .set_scissor_rect(0, 0, size.width, size.height);
    }
}

fn setup_renderer(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pipeline = UiTexturePipeline::new(&graphics_state);
    cmd.insert_resource(pipeline);
}

fn update_instances(
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<UiTexturePipeline>,
    mut ui: ResMut<super::UiState>,
    mut cmd: Commands,
    mut texture_rect_q: Query<(&mut TextureRectRequests, &mut UiScissor, EntityId)>,
) {
    fn update_draw_instances(
        w: f32,
        h: f32,
        instances: &mut HashMap<(u64, UiScissor), Vec<DrawRectInstance>>,
        rects: &TextureRectRequests,
        scissor: &UiScissor,
        pipeline: &UiTexturePipeline,
    ) {
        for rect in rects.0.iter() {
            let Some(atlas_rect) = pipeline.image_rects.get(&rect.image.id()) else {
                continue;
            };
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
                uv: atlas_rect.uv(),
            };
            instances
                .entry((atlas_rect.atlas_handle().id(), *scissor))
                .or_default()
                .push(instance);
        }
    }
    // TODO: retain buffer
    let w = renderer.size().x as f32;
    let h = renderer.size().y as f32;
    let mut instances = HashMap::<(AssetId, UiScissor), Vec<DrawRectInstance>>::default();

    let mut textured_rects = std::mem::take(&mut ui.texture_rects);
    textured_rects.sort_unstable_by_key(|r| r.scissor);

    let mut buffers_reused = 0;
    let mut rects_consumed = 0;
    for (g, (rects, sc, _id)) in
        (textured_rects.chunk_by_mut(|a, b| a.scissor == b.scissor)).zip(texture_rect_q.iter_mut())
    {
        buffers_reused += 1;
        rects_consumed += g.len();
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
        rects.0.clear();
        rects.0.extend(g.iter_mut().map(|x| std::mem::take(x)));
        update_draw_instances(w, h, &mut instances, rects, sc, &pipeline);
    }
    for (_, _, id) in texture_rect_q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in textured_rects[rects_consumed..].chunk_by_mut(|a, b| a.scissor == b.scissor) {
        let scissor = UiScissor(ui.scissors[g[0].scissor as usize]);
        let mut requests = TextureRectRequests(g.iter_mut().map(|x| std::mem::take(x)).collect());
        update_draw_instances(w, h, &mut instances, &requests, &scissor, &pipeline);
        cmd.spawn().insert_bundle((scissor, requests));
    }
    ui.texture_rects = textured_rects;

    // FIXME: retain buffers or do a smarter gc
    pipeline.instances.clear();
    for ((atlas_id, scissor), cpu) in instances.iter() {
        let rendering_data = pipeline.instances.entry(*scissor).or_default();

        let rendering_data = match rendering_data.iter_mut().find(|r| &r.atlas_id == atlas_id) {
            Some(x) => x,
            None => {
                let r = UiTextureRenderingInstances {
                    atlas_id: *atlas_id,
                    count: 0,
                    instance_gpu: renderer.device().create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!(
                            "Texture Instance Buffer - {:?} {}",
                            scissor, atlas_id
                        )),
                        mapped_at_creation: false,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        size: 0,
                    }),
                };
                rendering_data.push(r);
                rendering_data.last_mut().unwrap()
            }
        };

        let instance_data_bytes = bytemuck::cast_slice::<_, u8>(cpu.as_slice());
        let size = instance_data_bytes.len() as u64;
        if rendering_data.instance_gpu.size() < size {
            // resize the buffer
            rendering_data.instance_gpu =
                renderer.device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!(
                        "UI Texture Instance Buffer - {:?} {}",
                        scissor, atlas_id
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
        app.require_plugin(TextureAtlasPlugin);

        app.add_plugin(RenderCommandPlugin::<RectRenderCommand>::new(
            RenderPass::Ui,
        ));
        app.add_startup_system(setup_renderer);
        app.insert_resource(UiTextureReferences::default());
        app.with_stage(crate::Stage::PreUpdate, |s| {
            s.add_system(gc_text_textures);
        });
        app.with_stage(crate::Stage::PostUpdate, |s| {
            s.add_system(extract_textures)
                .add_system(update_instances.after(extract_textures));
        });
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
