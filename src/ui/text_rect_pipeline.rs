use std::collections::HashMap;
use std::mem::size_of;

use crate::assets::{AssetId, Assets, Handle, WeakHandle};
use crate::renderer::texture::texture_bind_group_layout;
use crate::renderer::texture_atlas::{
    AtlasRect, TextureAtlas, TextureAtlasPlugin, TextureAtlasRegistry,
};
use crate::renderer::{
    GraphicsState, RenderCommand, RenderCommandInput, RenderCommandPlugin, RenderPass, texture,
};
use crate::wgpu::include_wgsl;
use cecs::prelude::*;

use crate::Plugin;

use super::{ShapingResult, UiScissor};

#[derive(Default, Clone)]
pub struct TextRectRequests(pub Vec<DrawTextRect>);

/// XY are top-left corner, WH are full-extents
#[derive(Default, Clone, Debug)]
pub struct DrawTextRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub layer: u16,
    pub color: u32,
    pub shaping: Handle<ShapingResult>,
    pub scissor: u32,
}

/// XY is the center, WH are half-extents
#[derive(Debug, Default, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct DrawRectInstance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: u32,
    pub layer: f32,
    pub uv: [f32; 4],
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
                    offset: size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: (size_of::<[f32; 4]>() + size_of::<u32>()) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: (size_of::<[f32; 4]>() + size_of::<u32>() + size_of::<f32>())
                        as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

struct TextPipeline {
    text_rect_pipeline: wgpu::RenderPipeline,
    /// shaping_id -> AtlasRect
    shaping_rects: HashMap<AssetId<ShapingResult>, AtlasRect>,
    /// atlas_id -> bind group
    atlases: HashMap<AssetId<TextureAtlas>, wgpu::BindGroup>,
    instances: HashMap<UiScissor, Vec<UiTextureRenderingInstances>>,
}

pub struct UiTextureRenderingInstances {
    pub atlas_id: AssetId<TextureAtlas>,
    pub count: usize,
    pub instance_gpu: wgpu::Buffer,
}

#[derive(Default)]
struct UiTextureReferences(pub HashMap<AssetId<ShapingResult>, WeakHandle<super::ShapingResult>>);

fn gc_text_textures(
    mut texturerefs: ResMut<UiTextureReferences>,
    mut pipeline: ResMut<TextPipeline>,
    mut textures: TextureAtlasRegistry,
) {
    texturerefs.0.retain(|id, handle| {
        if handle.upgrade().is_none() {
            #[cfg(feature = "tracing")]
            tracing::debug!(?id, "Collecting expired text texture");
            if let Some(rect) = pipeline.shaping_rects.remove(id) {
                textures.deallocate(rect);
            }
            return false;
        }
        true
    });
}

fn extract_shaping_results(
    mut pipeline: ResMut<TextPipeline>,
    mut refs: ResMut<UiTextureReferences>,
    cache: Res<super::TextTextureCache>,
    shaping_results: Res<Assets<super::ShapingResult>>,
    mut textures: TextureAtlasRegistry,
) {
    for handle in cache.0.values() {
        let Some(res) = shaping_results.get(handle) else {
            continue;
        };
        let id = handle.id();
        if refs.0.contains_key(&id) {
            continue;
        }

        let Some(rect) = textures.allocate(res.texture.width() as i32, res.texture.height() as i32)
        else {
            #[cfg(feature = "tracing")]
            tracing::error!(
                id = ?handle.id(),
                "Failed to allocate texture for shaping result"
            );
            continue;
        };

        textures.upload_rgba(
            &rect,
            res.texture.pixmap.data(),
            res.texture.width(),
            res.texture.height(),
        );

        let atlas_id = rect.atlas_handle().id();
        pipeline.atlases.entry(atlas_id).or_insert_with(|| {
            let (_, texture_bind_group) = textures.get_bind_group(&rect);
            texture_bind_group
        });

        refs.0.insert(id, handle.downgrade());
        pipeline.shaping_rects.insert(id, rect);
    }
}

impl TextPipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let shader = renderer
            .device()
            .create_shader_module(include_wgsl!("ui-text.wgsl"));

        let texture_bind_group_layout =
            texture_bind_group_layout(renderer.device(), "ui-text-layout");

        let text_rect_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Ui Text Rect Render Pipeline"),
                    layout: Some(&renderer.device().create_pipeline_layout(
                        &wgpu::PipelineLayoutDescriptor {
                            label: Some("Ui Text Rect Render Pipeline Layout"),
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

        TextPipeline {
            text_rect_pipeline,
            shaping_rects: Default::default(),
            atlases: Default::default(),
            instances: Default::default(),
        }
    }
}

struct RectRenderCommand;

impl<'a> RenderCommand<'a> for RectRenderCommand {
    type Parameters = (Res<'a, crate::renderer::WindowSize>, Res<'a, TextPipeline>);

    fn render<'r>(
        input: &'r mut RenderCommandInput<'a, 'r>,
        (size, pipeline): &'r Self::Parameters,
    ) {
        input.render_pass.set_pipeline(&pipeline.text_rect_pipeline);

        for (scissor, requests) in pipeline.instances.iter() {
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
            for requests in requests.iter() {
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
    let pipeline = TextPipeline::new(&graphics_state);
    cmd.insert_resource(pipeline);
}

// preserve the buffers by zipping together a query with the chunks, spawn new if not enough,
// GC if too many
// most frames should have the same items
//
fn update_instances(
    renderer: Res<GraphicsState>,
    mut pipeline: ResMut<TextPipeline>,
    mut ui: ResMut<super::UiState>,
    mut cmd: Commands,
    mut q: Query<(&mut TextRectRequests, &mut UiScissor, EntityId)>,
) {
    // TODO: retain buffer
    let w = renderer.size().x as f32;
    let h = renderer.size().y as f32;
    let mut instances =
        HashMap::<(AssetId<TextureAtlas>, UiScissor), Vec<DrawRectInstance>>::default();
    let mut update_gpu_instances = |rects: &TextRectRequests, scissor| {
        for rect in rects.0.iter() {
            let Some(atlas_rect) = pipeline.shaping_rects.get(&rect.shaping.id()) else {
                continue;
            };

            let ww = rect.w as f32 * 0.5;
            let hh = rect.h as f32 * 0.5;
            // flip y
            let y = h - rect.y as f32;
            // switch order of layers, lower layers are in the front
            // remap to 0..1
            let layer = (0xFFFF - rect.layer) as f32 / (0xFFFF as f32);
            let instance = DrawRectInstance {
                x: (rect.x as f32 + ww) / w,
                y: (y - hh) / h,
                w: rect.w as f32 / w,
                h: rect.h as f32 / h,
                layer,
                color: rect.color,
                uv: atlas_rect.uv(),
            };

            let texture_atlas_id = atlas_rect.atlas_handle().id();
            instances
                .entry((texture_atlas_id, scissor))
                .or_default()
                .push(instance);
        }
    };

    let mut buffers_reused = 0;
    let mut rects_consumed = 0;
    // take the buffer so the borrow checker isn't panicking
    let mut text_rects = std::mem::take(&mut ui.text_rects);
    text_rects.sort_unstable_by_key(|r| r.scissor);
    for (g, (rects, sc, _id)) in
        (text_rects.chunk_by_mut(|a, b| a.scissor == b.scissor)).zip(q.iter_mut())
    {
        buffers_reused += 1;
        rects_consumed += g.len();
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
        rects.0.clear();
        rects.0.extend(g.iter_mut().map(|x| std::mem::take(x)));
        update_gpu_instances(rects, *sc);
    }
    for (_, _, id) in q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in text_rects[rects_consumed..].chunk_by_mut(|a, b| a.scissor == b.scissor) {
        let scissor = UiScissor(ui.scissors[g[0].scissor as usize]);
        let mut requests = TextRectRequests(g.iter_mut().map(|x| std::mem::take(x)).collect());
        update_gpu_instances(&mut requests, scissor);
        cmd.spawn().insert_bundle((scissor, requests));
    }
    // restore the buffer
    ui.text_rects = text_rects;

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
                            "Text Instance Buffer - {:?} {:?}",
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
                        "UI Text Instance Buffer - {:?} {:?}",
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

pub struct UiTextRectPlugin;

impl Plugin for UiTextRectPlugin {
    fn build(self, app: &mut crate::App) {
        app.insert_resource(TextRectRequests::default());
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
            s.add_system(extract_shaping_results)
                .add_system(update_instances.after(extract_shaping_results));
        });
    }
}
