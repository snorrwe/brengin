use crate::prelude::*;
use crate::renderer::texture::texture_bind_group_layout;
use crate::renderer::{texture, GraphicsState, RenderCommand, RenderCommandPlugin};
use crate::{App, DefaultPlugins, Plugin};
use glam::Vec3;
use wgpu::{include_wgsl, Texture};

pub struct BackgroundPlugin;

struct BackgroundPipeline {
    render_pipeline: wgpu::RenderPipeline,
    texture: Option<BackgroundTextureRenderingData>,
}

impl BackgroundPipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let texture_bind_group_layout =
            texture_bind_group_layout(&renderer.device, "background-texture-layout");

        let render_pipeline_layout =
            renderer
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("background-render-pipeline-layout"),
                    bind_group_layouts: &[&texture_bind_group_layout],
                    push_constant_ranges: &[],
                });

        let shader = renderer
            .device()
            .create_shader_module(include_wgsl!("background.wgsl"));

        let render_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("background-render-pipeline"),
                    layout: Some(&render_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
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
                        // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
                        polygon_mode: wgpu::PolygonMode::Fill,
                        // Requires Features::DEPTH_CLIP_CONTROL
                        unclipped_depth: false,
                        // Requires Features::CONSERVATIVE_RASTERIZATION
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

        Self {
            render_pipeline,
            texture: None,
        }
    }
}

impl<'a> RenderCommand<'a> for BackgroundPipeline {
    type Parameters = Res<'a, BackgroundPipeline>;
    fn render<'r>(
        crate::renderer::RenderCommandInput {
            render_pass,
            ..
        }: &'r mut crate::renderer::RenderCommandInput<'a, 'r>,

        pipeline: &'r Self::Parameters,
    ) {
        if let Some(d) = pipeline.texture.as_ref() {
            render_pass.set_bind_group(0, &d.texture_bind_group, &[]);
            render_pass.set_pipeline(&pipeline.render_pipeline);
            render_pass.draw(0..3, 0..1);
        }
    }
}

pub struct BackgroundTextureRenderingData {
    pub texture_bind_group: wgpu::BindGroup,
    pub texture: Texture,
}

impl Plugin for BackgroundPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(RenderCommandPlugin::<BackgroundPipeline>::new(
            crate::renderer::RenderPass::Background,
        ));
        if let Some(ref mut app) = app.render_app {
            app.add_startup_system(setup_pipeline);
        }
    }
}

fn setup_pipeline(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pl = BackgroundPipeline::new(&graphics_state);
    cmd.insert_resource(pl);
}
