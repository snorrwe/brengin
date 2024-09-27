use crate::renderer::{
    ExtractionPlugin, GraphicsState, RenderCommand, RenderCommandInput, RenderCommandPlugin,
    RenderPass,
};
use cecs::prelude::*;

use crate::{renderer::Extract, Plugin};

#[derive(Default, Clone, Debug)]
pub struct RectRequests(pub Vec<DrawRect>);

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

#[derive(Debug, Default, Clone, Copy)]
pub struct DrawRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub color: u32,
}

struct RectPipeline {}
impl RectPipeline {
    fn new(graphics_state: &GraphicsState) -> Self {
        Self {}
    }
}

struct RectRenderCommand;

impl<'a> RenderCommand<'a> for RectRenderCommand {
    type Parameters = (Query<'a, &'static RectRequests>, Res<'a, RectPipeline>);

    fn render<'r>(input: &'r mut RenderCommandInput<'a>, pipeline: &'r Self::Parameters) {}
}

fn setup(mut cmd: Commands) {
    cmd.spawn().insert(RectRequests::default());
}

fn setup_renderer(mut cmd: Commands, graphics_state: Res<GraphicsState>) {
    let pipeline = RectPipeline::new(&graphics_state);
    cmd.insert_resource(pipeline);
}

pub struct UiCorePlugin;

impl Plugin for UiCorePlugin {
    fn build(self, app: &mut crate::App) {
        app.add_startup_system(setup);
        app.add_plugin(ExtractionPlugin::<RectRequests>::default());

        app.add_plugin(RenderCommandPlugin::<RectRenderCommand>::new(
            RenderPass::Ui,
        ));
        if let Some(ref mut renderer) = app.render_app {
            renderer.add_startup_system(setup_renderer);
        }
    }
}
