use brengin::camera::{camera_bundle, PerspectiveCamera, WindowCamera};
use brengin::prelude::*;
use brengin::renderer::{texture, GraphicsState, RenderCommand, RenderCommandPlugin};
use brengin::{App, DefaultPlugins, Plugin};
use glam::Vec3;
use wgpu::include_wgsl;

struct GamePlugin;

fn setup(mut cmd: Commands) {
    //camera
    cmd.spawn()
        .insert(WindowCamera)
        .insert_bundle(camera_bundle(PerspectiveCamera {
            eye: Vec3::new(0.0, 0.0, 50.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            aspect: 16.0 / 9.0,
            fovy: std::f32::consts::TAU / 6.0,
            znear: 5.0,
            zfar: 5000.0,
        }))
        .insert_bundle(transform_bundle(Transform::default()));
}

struct MandelbrotPipeline {
    render_pipeline: wgpu::RenderPipeline,
}

impl MandelbrotPipeline {
    fn new(renderer: &GraphicsState) -> Self {
        let render_pipeline_layout =
            renderer
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Mandelbrot Render Pipeline Layout"),
                    bind_group_layouts: &[],
                    push_constant_ranges: &[],
                });

        let shader = renderer
            .device()
            .create_shader_module(include_wgsl!("assets/mandelbrot.wgsl"));

        let render_pipeline =
            renderer
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Mandelbrot Render Pipeline"),
                    layout: Some(&render_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[],
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

        Self { render_pipeline }
    }
}

impl<'a> RenderCommand<'a> for MandelbrotPipeline {
    type Parameters = Res<'a, MandelbrotPipeline>;
    fn render<'r>(
        brengin::renderer::RenderCommandInput {
            render_pass,
            ..
        }: &'r mut brengin::renderer::RenderCommandInput<'a>,

        pipeline: &'r Self::Parameters,
    ) {
        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.draw(0..3, 0..1);
    }
}

impl Plugin for GamePlugin {
    fn build(self, app: &mut brengin::App) {
        app.add_startup_system(setup);
        app.add_plugin(RenderCommandPlugin::<MandelbrotPipeline>::new(
            brengin::renderer::RenderPass::Ui,
        ));
        if let Some(ref mut app) = app.render_app {
            app.add_startup_system(setup_pipeline);
        }
    }
}

fn setup_pipeline(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pl = MandelbrotPipeline::new(&graphics_state);
    cmd.insert_resource(pl);
}

async fn game() {
    let mut app = App::default();
    app.add_plugin(DefaultPlugins);
    app.add_plugin(GamePlugin);
    app.run().await.unwrap();
}

fn main() {
    tracing_subscriber::fmt::init();
    pollster::block_on(game());
}
