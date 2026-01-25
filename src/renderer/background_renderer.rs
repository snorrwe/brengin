use crate::prelude::*;
use crate::renderer::texture::{texture_bind_group_layout, texture_to_bindings};
use crate::renderer::{texture, GraphicsState, RenderCommand, RenderCommandPlugin};
use crate::Plugin;
use image::DynamicImage;
use wgpu::include_wgsl;

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
                    ..Default::default()
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
                    multiview_mask: None,
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
    pub id: AssetId,
    pub texture_bind_group: wgpu::BindGroup,
    pub texture: texture::Texture,
}

pub struct BackgroundImage(pub Handle<DynamicImage>);

fn extract_background(
    mut pipeline: ResMut<BackgroundPipeline>,
    renderer: Res<GraphicsState>,
    img: Option<Res<BackgroundImage>>,
    images: Res<Assets<DynamicImage>>,
) {
    let Some(img) = img else {
        pipeline.texture.take();
        return;
    };
    let id = img.0.id();
    let img = images.get(&img.0);

    if let Some(t) = pipeline.texture.as_ref().map(|t| t.id) {
        if t == id {
            // texture is already registered
            return;
        }
        // new texture, clear the old one
        pipeline.texture.take();
    }
    let texture = texture::Texture::from_image(renderer.device(), renderer.queue(), img, None)
        .expect("Failed to create texture");

    let (_, texture_bind_group) = texture_to_bindings(&renderer.device, &texture);
    pipeline.texture = Some(BackgroundTextureRenderingData {
        id,
        texture,
        texture_bind_group,
    });
}

impl Plugin for BackgroundPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(RenderCommandPlugin::<BackgroundPipeline>::new(
            crate::renderer::RenderPass::Background,
        ));
        app.require_plugin(AssetsPlugin::<DynamicImage>::default());
        app.with_stage(crate::Stage::Update, |s| {
            s.add_system(extract_background);
        });
        app.add_startup_system(setup_pipeline);
    }
}

fn setup_pipeline(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pl = BackgroundPipeline::new(&graphics_state);
    cmd.insert_resource(pl);
}
