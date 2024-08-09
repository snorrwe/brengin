pub mod assets;
pub mod camera;
pub mod prelude;
pub mod quat_ext;
pub mod renderer;
pub mod transform;

#[cfg(feature = "audio")]
pub mod audio;

use anyhow::Context;
// reexport
pub use cecs;
pub use glam;
pub use image;
pub use wgpu;
pub use winit;

use winit::{
    application::ApplicationHandler,
    event::*,
    keyboard::{KeyCode, PhysicalKey},
    window::{Theme, WindowAttributes},
};

use std::{any::TypeId, collections::HashSet, sync::Arc};
use transform::TransformPlugin;

use renderer::{GraphicsState, RenderResult, RendererPlugin};

use winit::event_loop::EventLoop;

use cecs::prelude::*;

use tracing::{debug, error};

#[derive(Clone, Copy, Debug)]
pub struct Time(pub instant::Instant);

#[derive(Clone, Copy, Debug)]
pub struct DeltaTime(pub std::time::Duration);

#[derive(Clone, Debug)]
pub struct Timer {
    target: std::time::Duration,
    elapsed: std::time::Duration,
    repeat: bool,
    just_finished: bool,
}

impl Timer {
    pub fn new(duration: std::time::Duration, repeat: bool) -> Self {
        Self {
            target: duration,
            elapsed: Default::default(),
            repeat,
            just_finished: false,
        }
    }

    pub fn update(&mut self, dt: std::time::Duration) {
        self.just_finished = false;
        let finished = self.finished();

        self.elapsed += dt;

        if !finished && self.finished() {
            self.just_finished = true;
            if self.repeat {
                self.elapsed = Default::default();
            }
        }
    }

    pub fn finished(&self) -> bool {
        self.just_finished || self.elapsed >= self.target
    }

    pub fn just_finished(&self) -> bool {
        self.just_finished
    }

    pub fn reset(&mut self) {
        self.just_finished = false;
        self.elapsed = Default::default();
    }
}

fn update_time(mut time: ResMut<Time>, mut dt: ResMut<DeltaTime>) {
    let now = instant::Instant::now();
    dt.0 = now - time.0;
    time.0 = now;
}

pub struct App {
    world: World,
    stages: std::collections::BTreeMap<Stage, SystemStage<'static>>,
    startup_systems: SystemStage<'static>,
    plugins: HashSet<TypeId>,
}

/// The main reason behind requiring types instead of just functions is that a plugin may only be
/// registered once per `App`
pub trait Plugin {
    fn build(self, app: &mut App);
}

impl std::ops::Deref for App {
    type Target = World;

    fn deref(&self) -> &Self::Target {
        &self.world
    }
}

impl std::ops::DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.world
    }
}

impl Default for App {
    fn default() -> Self {
        let mut world = World::new(1024);
        world.insert_resource(WindowDescriptor::default());
        Self {
            world,
            stages: Default::default(),
            startup_systems: SystemStage::new("startup"),
            plugins: Default::default(),
        }
    }
}

pub struct Window(pub Arc<winit::window::Window>);

enum RunningApp {
    Pending(App),
    Initialized(World),
}

impl RunningApp {
    fn as_pending(&mut self) -> Option<&mut App> {
        if let Self::Pending(v) = self {
            Some(v)
        } else {
            None
        }
    }

    fn world_mut(&mut self) -> &mut World {
        match self {
            RunningApp::Pending(app) => &mut app.world,
            RunningApp::Initialized(w) => w,
        }
    }
}

impl ApplicationHandler for RunningApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(app) = self.as_pending() else {
            debug!("App resumed");
            return;
        };
        let attributes = app.world.get_resource_or_default::<WindowAttributes>();
        let window = event_loop.create_window(attributes.clone()).expect("Failed to create window");
        let window = Arc::new(window);
        // FIXME:
        // do not block here
        let graphics_state = pollster::block_on(GraphicsState::new(Arc::clone(&window)));

        app.world
            .run_system(|mut cmd: Commands| -> anyhow::Result<()> {
                cmd.spawn().insert(Window(Arc::clone(&window)));

                let sprite_pipeline =
                    renderer::sprite_renderer::SpritePipeline::new(&graphics_state);
                cmd.insert_resource(sprite_pipeline);
                cmd.insert_resource(graphics_state);

                anyhow::Result::Ok(())
            })
            .unwrap()
            .expect("Failed to create window");

        *self = RunningApp::Initialized(std::mem::take(app).build());
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        tracing::trace!(?event, "Event received");
        let world = match self {
            RunningApp::Pending(_) => return,
            RunningApp::Initialized(w) => w,
        };
        match event {
            #[cfg(not(target_family = "wasm"))]
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    },
                ..
            } => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                world
                    .run_system(move |mut state: ResMut<GraphicsState>| {
                        state.resize(size);
                    })
                    .unwrap();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                world
                    .get_resource_mut::<KeyBoardInputs>()
                    .unwrap()
                    .next
                    .push(event.clone());
            }
            WindowEvent::RedrawRequested => {
                world.tick();

                let result = world.get_resource::<RenderResult>();
                if let Some(result) = result {
                    match result {
                        Ok(_) => {}
                        // handled by the renderer system
                        Err(wgpu::SurfaceError::Lost) => {}
                        // The system is out of memory, we should probably quit
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            error!("gpu out of memory");
                            event_loop.exit();
                        }
                        // All other errors (Outdated, Timeout) should be resolved by the next frame
                        Err(e) => debug!("rendering failed: {:?}", e),
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.world_mut()
            .run_system(|q: Query<&Window>| {
                for Window(window) in q.iter() {
                    window.request_redraw();
                }
            })
            .unwrap();
    }
}

impl App {
    pub fn add_plugin<T: Plugin + 'static>(&mut self, plugin: T) -> &mut Self {
        let id = TypeId::of::<T>();
        assert!(
            !self.plugins.contains(&id),
            "Plugins can be only registered once"
        );
        self.plugins.insert(id);
        plugin.build(self);
        self
    }

    pub fn with_stage(
        &mut self,
        stage: Stage,
        f: impl FnOnce(&mut SystemStage<'static>),
    ) -> &mut Self {
        let stage = self
            .stages
            .entry(stage)
            .or_insert_with(move || SystemStage::new(format!("Stage-{:?}", stage)));
        f(
            // # SAFETY
            // No fucking idea, but I can't decypher the bloody compiler error so here we are
            unsafe { std::mem::transmute::<&mut SystemStage, &mut SystemStage>(stage) },
        );
        self
    }

    pub fn add_startup_system<P>(
        &mut self,
        sys: impl cecs::systems::IntoSystem<'static, P, ()>,
    ) -> &mut Self {
        self.startup_systems.add_system(sys);
        self
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new().context("Failed to initialize EventLoop")?;

        let window = self.world.run_view_system(|desc: Res<WindowDescriptor>| {
            WindowAttributes::default()
                .with_title(&desc.title)
                .with_fullscreen(desc.fullscreen.clone())
                .with_theme(Some(Theme::Dark))
        });

        self.world.insert_resource(window);

        let mut app = RunningApp::Pending(self);
        event_loop.run_app(&mut app)?;

        Ok(())
    }

    fn build(self) -> World {
        let mut world = self.world;
        for (_, stage) in self.stages {
            world.add_stage(stage);
        }
        world.run_stage(self.startup_systems).unwrap();
        world.vacuum();
        world
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    PreUpdate = 1,
    Update = 2,
    PostUpdate = 3,
    Transform = 4,
    Render = 5,
}

#[derive(Default)]
pub struct KeyBoardInputs {
    pub inputs: Vec<KeyEvent>,
    pub(crate) next: Vec<KeyEvent>,
    pub pressed: HashSet<KeyCode>,
    pub just_released: HashSet<KeyCode>,
    pub just_pressed: HashSet<KeyCode>,
}

impl KeyBoardInputs {
    pub fn update(&mut self) {
        std::mem::swap(&mut self.inputs, &mut self.next);
        self.next.clear();
        self.just_released.clear();
        self.just_pressed.clear();
        for k in self.inputs.iter() {
            match k.state {
                ElementState::Pressed => {
                    if let PhysicalKey::Code(k) = k.physical_key {
                        if !self.pressed.contains(&k) {
                            self.just_pressed.insert(k);
                        }
                        self.pressed.insert(k);
                    }
                }
                ElementState::Released => {
                    if let PhysicalKey::Code(k) = k.physical_key {
                        self.pressed.remove(&k);
                        self.just_released.insert(k);
                    }
                }
            }
        }
    }
}

fn update_inputs(mut k: ResMut<KeyBoardInputs>) {
    k.update();
}

pub struct DefaultPlugins;

impl Plugin for DefaultPlugins {
    fn build(self, app: &mut App) {
        // TODO: input plugin, time plugin
        app.insert_resource(Time(instant::Instant::now()));
        app.insert_resource(DeltaTime(std::time::Duration::default()));
        app.insert_resource(KeyBoardInputs::default());

        app.with_stage(Stage::PreUpdate, |s| {
            s.add_system(update_time).add_system(update_inputs);
        });

        app.add_plugin(assets::AssetsPlugin::<renderer::sprite_renderer::SpriteSheet>::default());
        app.add_plugin(TransformPlugin);
        app.add_plugin(RendererPlugin);

        #[cfg(feature = "audio")]
        app.add_plugin(audio::AudioPlugin);
    }
}

#[derive(Debug, Clone)]
pub struct WindowDescriptor {
    pub title: String,
    pub fullscreen: Option<winit::window::Fullscreen>,
}

// for MacOS:
// winit Fullscreen contains a c_void pointer
unsafe impl Send for WindowDescriptor {}
unsafe impl Sync for WindowDescriptor {}

impl Default for WindowDescriptor {
    fn default() -> Self {
        Self {
            title: "brengin".to_string(),
            fullscreen: None,
        }
    }
}
