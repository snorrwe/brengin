use std::mem::size_of;

use crate::renderer::{
    texture, ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput,
    RenderCommandPlugin, RenderPass,
};
use crate::wgpu::include_wgsl;
use cecs::prelude::*;
use wgpu::util::DeviceExt as _;

use crate::{renderer::Extract, Plugin};

use super::UiScissor;

#[derive(Default, Clone, Debug)]
pub struct RectRequests(pub Vec<DrawColorRect>);

struct RectInstanceBuffer {
    buffer: wgpu::Buffer,
    len: usize,
    capacity: usize,
}

impl Extract for RectRequests {
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
#[derive(Debug, Default, Clone, Copy)]
pub struct DrawColorRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub layer: u16,
    pub color: u32,
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
                    offset: size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: (size_of::<[f32; 4]>() + size_of::<u32>()) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

struct RectPipeline {
    color_rect_pipeline: wgpu::RenderPipeline,
}

impl RectPipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let shader = renderer
            .device()
            .create_shader_module(include_wgsl!("ui-rect.wgsl"));

        let color_rect_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Ui Color Rect Render Pipeline"),
                    layout: Some(&renderer.device().create_pipeline_layout(
                        &wgpu::PipelineLayoutDescriptor {
                            label: Some("Ui Color Rect Render Pipeline Layout"),
                            bind_group_layouts: &[],
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
                        alpha_to_coverage_enabled: false,
                    },
                    multiview: None,
                    cache: None,
                });

        RectPipeline {
            color_rect_pipeline,
        }
    }
}

struct RectRenderCommand;

impl<'a> RenderCommand<'a> for RectRenderCommand {
    type Parameters = (
        Query<'a, (&'static RectInstanceBuffer, &'static UiScissor)>,
        Res<'a, crate::renderer::WindowSize>,
        Res<'a, RectPipeline>,
    );

    fn render<'r>(
        input: &'r mut RenderCommandInput<'a>,
        (rects, size, pipeline): &'r Self::Parameters,
    ) {
        input
            .render_pass
            .set_pipeline(&pipeline.color_rect_pipeline);
        for (requests, scissor) in rects.iter() {
            let x = scissor.0.min_x.max(0) as u32;
            let y = scissor.0.min_y.max(0) as u32;
            let w = (scissor.0.width() as u32).min(size.width.saturating_sub(x));
            let h = (scissor.0.height() as u32).min(size.height.saturating_sub(y));

            if w == 0 || h == 0 {
                tracing::warn!(?scissor, "Scissor is outside of render target {:?}", **size);
                continue;
            }

            input.render_pass.set_scissor_rect(x, y, w, h);
            input
                .render_pass
                .set_vertex_buffer(0, requests.buffer.slice(..));
            input.render_pass.draw(0..6, 0..requests.len as u32);
        }
    }
}

fn setup_renderer(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pipeline = RectPipeline::new(&graphics_state);
    cmd.insert_resource(pipeline);
}

fn update_instances(
    mut q: Query<(EntityId, &RectRequests, Option<&mut RectInstanceBuffer>)>,
    renderer: Res<GraphicsState>,
    mut cmd: Commands,
) {
    // TODO: retain buffer
    let mut buff = Vec::new();
    let w = renderer.size().x as f32;
    let h = renderer.size().y as f32;
    for (id, rects, buffer) in q.iter_mut() {
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
            Some(buffer) if rects.0.len() <= buffer.capacity => {
                renderer.queue().write_buffer(
                    &buffer.buffer,
                    0,
                    bytemuck::cast_slice(buff.as_slice()),
                );
                buffer.len = buff.len();
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
                    len: rects.0.len(),
                    capacity: rects.0.len(),
                });
            }
        }
    }
}

pub struct UiColorRectPlugin;

impl Plugin for UiColorRectPlugin {
    fn build(self, app: &mut crate::App) {
        app.insert_resource(RectRequests::default());
        app.add_plugin(ExtractionPlugin::<RectRequests>::default());

        app.add_plugin(RenderCommandPlugin::<RectRenderCommand>::new(
            RenderPass::Ui,
        ));
        if let Some(ref mut renderer) = app.render_app {
            renderer.add_startup_system(setup_renderer);
            renderer.with_stage(crate::Stage::Update, |s| {
                s.add_system(update_instances);
            });
        }
    }
}
