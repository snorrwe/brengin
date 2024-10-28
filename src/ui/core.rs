use std::mem::size_of;

use crate::renderer::{
    ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput, RenderCommandPlugin,
    RenderPass,
};
use crate::wgpu::include_wgsl;
use cecs::prelude::*;
use wgpu::util::DeviceExt as _;

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
            ],
        }
    }
}

struct RectPipeline {
    render_pipeline: wgpu::RenderPipeline,
    ui_rect_layout: wgpu::BindGroupLayout,
}

impl RectPipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let ui_rect_layout: wgpu::BindGroupLayout =
            renderer
                .device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Ui Rect Uniform Layout"),
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

        let render_pipeline_layout =
            renderer
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Ui Rect Render Pipeline Layout"),
                    bind_group_layouts: &[],
                    push_constant_ranges: &[],
                });
        let render_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Ui Rect Render Pipeline"),
                    layout: Some(&render_pipeline_layout),
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
                    depth_stencil: None,
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
            render_pipeline,
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
        input.render_pass.set_pipeline(&pipeline.render_pipeline);
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
            DrawRectInstance {
                x: (rect.x as f32 + ww) / w,
                y: (y - hh) / h,
                // w: ww / w,
                w: rect.w as f32 / w,
                h: rect.h as f32 / h,
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
        if let Some(ref mut renderer) = app.render_app {
            renderer.add_startup_system(setup_renderer);
            renderer.with_stage(crate::Stage::Update, |s| {
                s.add_system(update_instances);
            });
        }
    }
}
