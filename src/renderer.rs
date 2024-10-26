pub mod sprite_renderer;
pub mod texture;

use std::{collections::BTreeSet, marker::PhantomData, sync::Arc};

use cecs::{
    prelude::*,
    query::{filters::Filter, QueryFragment, WorldQuery},
    Component,
};
use tracing::debug;
use wgpu::{Backends, InstanceFlags, StoreOp};
use winit::{dpi::PhysicalSize, window::Window};

pub use crate::camera::camera_bundle;
use crate::{
    camera::{CameraBuffer, CameraPlugin, CameraUniform},
    ExtractionTick, GameWorld, Plugin,
};

use self::sprite_renderer::SpriteRendererPlugin;

pub struct GraphicsState {
    pub clear_color: wgpu::Color,

    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,

    camera_bind_group_layout: wgpu::BindGroupLayout,

    depth_texture: texture::Texture,
}

#[derive(Debug, Default, Clone)]
pub struct RenderPasses(pub BTreeSet<RenderPass>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct RenderCommandInput<'a> {
    pub render_pass: &'a mut wgpu::RenderPass<'a>,
    pub camera: &'a wgpu::BindGroup,
}

pub trait RenderCommand<'a> {
    type Parameters: WorldQuery<'a>;

    fn render<'r>(input: &'r mut RenderCommandInput<'a>, params: &'r Self::Parameters);
}

#[derive(Clone)]
struct RenderCommandInternal {
    pub render_cmd: Arc<dyn Fn(&World, &mut RenderCommandInput) + Send + Sync>,
    pub pass: RenderPass,
}

impl RenderCommandInternal {
    pub fn new<T: RenderCommand<'static> + 'static>(pass: RenderPass) -> Self {
        Self {
            render_cmd: Arc::new(move |world, input| {
                world.run_view_system(move |q: T::Parameters| unsafe {
                    let q: &T::Parameters = std::mem::transmute(&q);
                    let input: &mut RenderCommandInput<'_> = std::mem::transmute(input);
                    T::render(input, q);
                });
            }),
            pass,
        }
    }
}

impl Extract for RenderCommandInternal {
    type QueryItem = &'static Self;
    type Filter = ();
    type Out = (Self,);

    fn extract<'a>(it: <Self::QueryItem as QueryFragment>::Item<'a>) -> Option<Self::Out> {
        Some((it.clone(),))
    }
}

/// RenderCommands are ran on the Render World
pub struct RenderCommandPlugin<T> {
    pub pass: RenderPass,
    _m: PhantomData<T>,
}

impl<T> RenderCommandPlugin<T> {
    pub fn new(pass: RenderPass) -> Self {
        Self {
            pass,
            _m: PhantomData,
        }
    }
}

impl<T> Plugin for RenderCommandPlugin<T>
where
    T: RenderCommand<'static> + 'static,
{
    fn build(self, app: &mut crate::App) {
        app.get_resource_or_default::<RenderPasses>()
            .0
            .insert(self.pass);
        let pass = self.pass;
        app.add_startup_system(move |mut cmd: Commands| {
            cmd.spawn().insert(RenderCommandInternal::new::<T>(pass));
        });
    }
}

impl GraphicsState {
    pub async fn new(window: Arc<Window>) -> Self {
        #[cfg(not(debug_assertions))]
        let flags = InstanceFlags::default();
        #[cfg(debug_assertions)]
        let flags = InstanceFlags::debugging();

        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: Backends::all(),
            dx12_shader_compiler: Default::default(),
            flags,
            gles_minor_version: Default::default(),
        });
        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to create adapter");

        debug!("Choosen adapter: {:?}", adapter);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    // WebGL doesn't support all of wgpu's features, so if
                    // we're building for the web we'll have to disable some.
                    required_limits: if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
                    label: None,
                    // TODO: lett application control this
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None, // Trace path
            )
            .await
            .unwrap();

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            view_formats: vec![surface.get_capabilities(&adapter).formats[0]],
            width: size.width.max(1),
            height: size.height.max(1),
            // TODO: configure
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            // TODO: configure
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let camera_bind_group_layout = device.create_bind_group_layout(&CameraUniform::desc());

        let depth_texture =
            texture::Texture::create_depth_texture(&device, &config, "depth_texture");

        Self {
            depth_texture,
            size,
            device,
            queue,
            config,
            surface,
            camera_bind_group_layout,
            clear_color: wgpu::Color {
                r: 0.4588,
                g: 0.031,
                b: 0.451,
                a: 1.0,
            },
            window,
        }
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.depth_texture =
                texture::Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
        }
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn surface(&self) -> &wgpu::Surface {
        &self.surface
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub fn size_mut(&mut self) -> &mut PhysicalSize<u32> {
        &mut self.size
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn camera_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.camera_bind_group_layout
    }

    pub fn depth_texture(&self) -> &texture::Texture {
        &self.depth_texture
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}

impl Vertex {
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            // attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2, // NEW!
                },
            ],
        }
    }
}

pub type RenderResult = Result<(), wgpu::SurfaceError>;

pub struct RendererPlugin;

fn extract_passes(mut cmd: Commands, game_world: Res<GameWorld>) {
    cmd.insert_resource(
        game_world
            .world()
            .get_resource::<RenderPasses>()
            .cloned()
            .unwrap_or_default(),
    )
}

impl Plugin for RendererPlugin {
    fn build(self, app: &mut crate::App) {
        app.render_app_mut().with_stage(crate::Stage::Render, |s| {
            s.add_system(render_system);
        });
        app.insert_resource(WindowSize {
            width: 0,
            height: 0,
        });
        app.add_extract_system(extract_passes);
        app.add_plugin(CameraPlugin);
        app.add_plugin(SpriteRendererPlugin);
        app.add_plugin(ExtractionPlugin::<RenderCommandInternal>::default());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderPass {
    Transparent = 4,
    Ui = 5,
}

impl RenderPass {
    fn begin<'a>(
        self,
        view: &wgpu::TextureView,
        encoder: &'a mut wgpu::CommandEncoder,
        state: &GraphicsState,
    ) -> wgpu::RenderPass<'a> {
        match self {
            RenderPass::Transparent => self.begin_transparent(view, encoder, state),
            RenderPass::Ui => self.begin_ui(view, encoder, state),
        }
    }

    fn begin_ui<'a>(
        self,
        view: &wgpu::TextureView,
        encoder: &'a mut wgpu::CommandEncoder,
        _state: &GraphicsState,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("UI Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    fn begin_transparent<'a>(
        self,
        view: &wgpu::TextureView,
        encoder: &'a mut wgpu::CommandEncoder,
        state: &GraphicsState,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Transparent Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &state.depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }
}

fn render_system(mut world: WorldAccess) {
    let w = world.world();
    let result = w.run_view_system(
        |state: Res<GraphicsState>,
         render_passes: Option<Res<RenderPasses>>,
         cameras: Query<&CameraBuffer>,
         render_commands: Query<&RenderCommandInternal>| {
            let Some(render_passes) = render_passes else {
                tracing::trace!("No render pass has been registered");
                return Ok(());
            };
            let cameras = cameras.iter();
            let output = state.surface.get_current_texture()?;
            let view = output
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder =
                state
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Render Encoder"),
                    });

            // clear
            {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(state.clear_color),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                });
            }
            for camera_buffer in cameras {
                // FIXME: retain the camera bind ground
                let camera_bind_group =
                    state.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        layout: &state.camera_bind_group_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: camera_buffer.0.as_entire_binding(),
                        }],
                        label: Some("camera_bind_group"),
                    });
                for pass in render_passes.0.iter() {
                    let mut render_pass = pass.begin(&view, &mut encoder, &state);
                    let mut input = RenderCommandInput {
                        render_pass: &mut render_pass,
                        camera: &camera_bind_group,
                    };
                    for cmd in render_commands.iter().filter(|p| &p.pass == pass) {
                        (cmd.render_cmd)(w, &mut input);
                    }
                }
            }

            state.queue.submit(std::iter::once(encoder.finish()));
            output.present();

            Ok(())
        },
    );
    let w = world.world_mut();
    // Reconfigure the surface if lost
    if let Err(wgpu::SurfaceError::Lost) = result {
        let state = w.get_resource_mut::<GraphicsState>().unwrap();
        let size = state.size();
        state.resize(size);
    }
    w.insert_resource(result);
}

pub trait Extract: Component {
    type QueryItem: QueryFragment + 'static;
    type Filter: Filter + 'static;
    type Out: Bundle;

    fn extract(it: <Self::QueryItem as QueryFragment>::Item<'_>) -> Option<Self::Out>;
}

fn extractor_system<T: Extract>(
    mut cmd: Commands,
    game_world: Res<GameWorld>,
    tick: Res<ExtractionTick>,
) where
    Query<'static, (EntityId, T::QueryItem), T::Filter>: cecs::query::WorldQuery<'static>,
{
    game_world
        .world()
        .run_view_system(|q: Query<(EntityId, T::QueryItem), T::Filter>| {
            for (id, q) in q.iter() {
                if let Some(out) = <T as Extract>::extract(q) {
                    cmd.insert_id(id).insert(*tick).insert_bundle(out);
                }
            }
        });
}

fn gc_system<T: Extract>(
    mut cmd: Commands,
    q: Query<(EntityId, &ExtractionTick)>,
    tick: Res<ExtractionTick>,
) {
    for (id, t) in q.iter() {
        if t != &*tick {
            cmd.delete(id);
        }
    }
}

pub struct ExtractionPlugin<T> {
    _m: PhantomData<T>,
}

impl<T> Default for ExtractionPlugin<T> {
    fn default() -> Self {
        Self { _m: PhantomData }
    }
}

impl<T> Plugin for ExtractionPlugin<T>
where
    T: Extract,
{
    fn build(self, app: &mut crate::App) {
        app.add_extract_system(extractor_system::<T>);
        app.render_app_mut().with_stage(crate::Stage::Update, |s| {
            s.add_system(gc_system::<T>);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;

    use super::*;

    struct TestRenderComponent {
        pub i: i32,
        pub j: u32,
    }

    impl Extract for TestRenderComponent {
        type QueryItem = (&'static i32, &'static u32);

        type Filter = ();

        type Out = (Self,);

        fn extract((i, j): <Self::QueryItem as QueryFragment>::Item<'_>) -> Option<Self::Out> {
            Some((Self { i: *i, j: *j },))
        }
    }

    #[test]
    fn test_extract_basic() {
        let mut game_world = World::new(4);
        game_world
            .run_system(|mut cmd: Commands| {
                cmd.spawn().insert_bundle((42i32, 32u32));
            })
            .unwrap();

        let mut render_world = World::new(4);
        render_world.insert_resource(GameWorld {
            world: NonNull::new(&mut game_world).unwrap(),
        });
        render_world.insert_resource(ExtractionTick(0));

        // inserts should be idempotent
        for _ in 0..5 {
            render_world
                .run_system(extractor_system::<TestRenderComponent>)
                .unwrap();
        }

        render_world.run_view_system(|q: Query<&TestRenderComponent>| {
            let mut n = 0;
            for i in q.iter() {
                assert_eq!(i.i, 42);
                assert_eq!(i.j, 32);
                n += 1
            }
            assert_eq!(n, 1);
        });
    }
}
