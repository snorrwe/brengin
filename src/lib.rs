#![feature(debug_closure_helpers)]

pub mod assets;
pub mod camera;
pub mod prelude;
pub mod quat_ext;
pub mod renderer;
pub mod transform;

#[cfg(feature = "audio")]
pub mod audio;
pub mod color;
pub mod ui;

use anyhow::Context;
use instant::Instant;
use ui::UiPlugin;

// reexport
pub use cecs;
pub use glam;
pub use image;
pub use parking_lot;
pub use wgpu;
pub use winit;

use winit::{
    application::ApplicationHandler,
    event::*,
    keyboard::{KeyCode, PhysicalKey},
    window::{Theme, WindowAttributes},
};

use parking_lot::Mutex;
use std::{
    any::{type_name, TypeId},
    collections::{HashMap, HashSet},
    sync::{atomic::AtomicBool, Arc},
    thread::JoinHandle,
    time::Duration,
};
use transform::TransformPlugin;

use renderer::{GraphicsState, RenderResult, RendererPlugin, WindowSize};

use winit::event_loop::EventLoop;

use cecs::{prelude::*, systems::SystemStageBuilder};

#[derive(Clone, Copy, Debug)]
pub struct Time(pub instant::Instant);

#[derive(Clone, Copy, Debug)]
pub struct DeltaTime(pub std::time::Duration);

#[derive(Clone, Debug)]
pub struct Timer {
    target: std::time::Duration,
    elapsed: std::time::Duration,
    pub repeat: bool,
    just_finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionTick(pub u32);

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

    pub fn period(&self) -> Duration {
        self.target
    }

    pub fn reset_period(&mut self, target: Duration) {
        self.target = target;
        self.reset();
    }

    pub fn percent(&self) -> f32 {
        let t = self.elapsed.as_secs_f32() / self.target.as_secs_f32();
        t.clamp(0.0, 1.0)
    }
}

fn update_time(mut time: ResMut<Time>, mut dt: ResMut<DeltaTime>, mut tick: ResMut<Tick>) {
    let now = instant::Instant::now();
    dt.0 = now - time.0;
    time.0 = now;
    tick.0 += 1;
}

pub struct App {
    world: World,
    stages: std::collections::BTreeMap<Stage, SystemStageBuilder<'static>>,
    startup_systems: SystemStageBuilder<'static>,
    plugins: HashSet<TypeId>,
    /// after build assert that these plugins are available
    /// store id: type name
    required_plugins: HashMap<TypeId, &'static str>,
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
        App::empty()
    }
}

enum RunningApp {
    Pending(App),
    Initialized {
        render_stage: SystemStage<'static>,
        game_world: Arc<Mutex<World>>,
        game_thread: JoinHandle<()>,
        enabled: Arc<AtomicBool>,
    },
    Terminated,
}

impl RunningApp {
    fn as_pending(&mut self) -> Option<&mut App> {
        if let Self::Pending(v) = self {
            Some(v)
        } else {
            None
        }
    }

    fn stop(&mut self) {
        if let RunningApp::Initialized {
            game_thread,
            enabled,
            ..
        } = std::mem::replace(self, RunningApp::Terminated)
        {
            enabled.store(false, std::sync::atomic::Ordering::Relaxed);
            game_thread.join().expect("Failed to join game thread");
        }
    }

    /// panics if self is not Pending
    fn initialize_pending(&mut self, graphics_state: GraphicsState) {
        let app = self.as_pending().unwrap();
        app.insert_resource(graphics_state);

        let InitializedWorlds {
            mut game_world,
            render_stage,
        } = std::mem::take(app).build();

        let (close_tx, close_rx) = crossbeam::channel::bounded(4);
        game_world.insert_resource(CloseRequest { tx: close_tx });
        game_world.insert_resource(CloseRequestRx { rx: close_rx });

        let game_world = Arc::new(Mutex::new(game_world));
        let enabled = Arc::new(AtomicBool::new(true));
        let game_thread = std::thread::spawn({
            let game_world = Arc::clone(&game_world);
            let enabled = Arc::clone(&enabled);
            move || game_thread(game_world, enabled)
        });

        *self = RunningApp::Initialized {
            game_world,
            game_thread,
            render_stage,
            enabled,
        };
    }
}

fn game_thread(game_world: Arc<Mutex<World>>, enabled: Arc<AtomicBool>) {
    // TODO: take from resource
    let target_frame_latency: Duration = Duration::from_millis(15);
    // reset Time so the first DT isn't outragous
    game_world
        .lock()
        .insert_resource(Time(instant::Instant::now()));
    while enabled.load(std::sync::atomic::Ordering::Relaxed) {
        let start = Instant::now();

        let mut game_world = game_world.lock();
        game_world.tick();
        drop(game_world);

        let end = Instant::now();
        let frame_duration = end - start;
        let sleep = if frame_duration < target_frame_latency {
            target_frame_latency - frame_duration
        } else {
            // leave time for the render thread to extract even if we're behind schedule
            Duration::from_micros(500)
        };
        std::thread::sleep(sleep);
    }
}

pub struct CloseRequest {
    tx: crossbeam::channel::Sender<()>,
}

impl CloseRequest {
    pub fn request_close(&self) {
        self.tx
            .send(())
            .inspect_err(|_err| {
                #[cfg(feature = "tracing")]
                tracing::error!(?_err, "Failed to send close request");
            })
            .unwrap_or(());
    }
}

struct CloseRequestRx {
    rx: crossbeam::channel::Receiver<()>,
}

impl ApplicationHandler for RunningApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(app) = self.as_pending() else {
            return;
        };
        let attributes = app.world.get_resource_or_default::<WindowAttributes>();
        let window = event_loop
            .create_window(attributes.clone())
            .expect("Failed to create window");
        let window = Arc::new(window);
        // FIXME:
        // do not block here
        let size = window.inner_size();
        let graphics_state = pollster::block_on(GraphicsState::new(
            Arc::clone(&window),
            glam::UVec2 {
                x: size.width,
                y: size.height,
            },
        ));

        app.insert_resource(window);

        self.initialize_pending(graphics_state);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        #[cfg(feature = "tracing")]
        tracing::trace!(?event, "Event received");
        let RunningApp::Initialized {
            game_world,
            render_stage,
            ..
        } = self
        else {
            return;
        };
        match event {
            #[cfg(not(target_family = "wasm"))]
            WindowEvent::CloseRequested => {
                #[cfg(feature = "tracing")]
                tracing::info!("Close requested. Stopping game loop");
                self.stop();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                game_world
                    .lock()
                    .run_system(move |mut state: ResMut<GraphicsState>, mut cmd: Commands| {
                        cmd.insert_resource(WindowSize {
                            width: size.width,
                            height: size.height,
                        });

                        state.resize(glam::UVec2 {
                            x: size.width,
                            y: size.height,
                        });
                    })
                    .unwrap();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                // TODO: have a shared lock-free input queue to get rid of this lock
                game_world
                    .lock()
                    .get_resource_mut::<KeyBoardInputs>()
                    .unwrap()
                    .next
                    .push(event.clone());
            }
            WindowEvent::MouseInput { state, button, .. } => {
                game_world
                    .lock()
                    .get_resource_mut::<MouseInputs>()
                    .unwrap()
                    .next
                    .push((button, state));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                game_world
                    .lock()
                    .get_resource_mut::<MouseInputs>()
                    .unwrap()
                    .next_scroll
                    .push(delta);
            }
            WindowEvent::CursorMoved { position, .. } => {
                game_world
                    .lock()
                    .get_resource_mut::<MouseInputs>()
                    .unwrap()
                    .cursor_position = position;
            }
            WindowEvent::RedrawRequested => {
                let mut world = game_world.lock();
                if world
                    .get_resource::<CloseRequestRx>()
                    .map(|CloseRequestRx { rx }| rx.try_recv().is_ok())
                    .unwrap_or(false)
                {
                    event_loop.exit();
                    return;
                }

                world.run_stage(render_stage.clone());

                let result = world.get_resource::<RenderResult>();
                if let Some(result) = result {
                    match result {
                        Ok(_) => {}
                        // handled by the renderer system
                        Err(wgpu::SurfaceError::Lost) => {
                            #[cfg(feature = "tracing")]
                            tracing::info!("Surface lost")
                        }
                        // The system is out of memory, we should probably quit
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            drop(world);
                            #[cfg(feature = "tracing")]
                            tracing::error!("gpu out of memory");
                            self.stop();
                            event_loop.exit();
                        }
                        // All other errors (Outdated, Timeout) should be resolved by the next frame
                        Err(_e) => {
                            #[cfg(feature = "tracing")]
                            tracing::info!(error=?_e,"rendering failed")
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        #[cfg(feature = "tracing")]
        tracing::trace!("• about_to_wait");
        if let RunningApp::Terminated = self {
            #[cfg(feature = "tracing")]
            tracing::trace!("x about_to_wait");
            return;
        }
        let sys = |window: Res<Arc<winit::window::Window>>| {
            #[cfg(feature = "tracing")]
            tracing::trace!("redraw {:?}", &*window);
            window.request_redraw();
        };
        match self {
            RunningApp::Pending(app) => app.world.run_view_system(sys),
            RunningApp::Initialized { game_world, .. } => game_world.lock().run_view_system(sys),
            RunningApp::Terminated => unreachable!(),
        }
        #[cfg(feature = "tracing")]
        tracing::trace!("✓ about_to_wait");
    }
}

impl App {
    fn empty() -> Self {
        let mut world = World::new(1024);
        world.insert_resource(WindowDescriptor::default());
        Self {
            world,
            stages: Default::default(),
            startup_systems: SystemStage::new("startup"),
            plugins: Default::default(),
            required_plugins: Default::default(),
        }
    }

    /// Panics if repeatedly called with the same type.
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

    /// Adds the plugin the first time it's called. If called repeatedly with the same type
    /// repeated calls are a noop.
    pub fn require_plugin<T: Plugin + 'static>(&mut self, plugin: T) -> &mut Self {
        if self.plugins.insert(TypeId::of::<T>()) {
            plugin.build(self);
        }
        self
    }

    /// After the whole App has been built, assert that the specified plugin has been added
    pub fn assert_plugin<T: Plugin + 'static>(&mut self) -> &mut Self {
        self.required_plugins
            .insert(TypeId::of::<T>(), type_name::<T>());
        self
    }

    pub fn with_stage(
        &mut self,
        stage: Stage,
        f: impl FnOnce(&mut SystemStageBuilder<'static>),
    ) -> &mut Self {
        let stage = self
            .stages
            .entry(stage)
            .or_insert_with(move || SystemStageBuilder::new(format!("Stage-{:?}", stage)));
        f(
            // # SAFETY
            // No fucking idea, but I can't decypher the bloody compiler error so here we are
            unsafe {
                std::mem::transmute::<&mut SystemStageBuilder, &mut SystemStageBuilder>(stage)
            },
        );
        self
    }

    /// Nest another cecs stage in the given `stage`
    pub fn with_nested_stage(&mut self, stage: Stage, s: SystemStageBuilder<'static>) -> &mut Self {
        let stage = self
            .stages
            .entry(stage)
            .or_insert_with(move || SystemStageBuilder::new(format!("Stage-{:?}", stage)));

        stage.add_nested_stage(s);
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

    fn _build(self) -> World {
        for (id, name) in self.required_plugins {
            assert!(
                self.plugins.contains(&id),
                "Plugin {name} is marked required but has not been initialized",
            );
        }

        let mut world = self.world;
        for (_, stage) in self
            .stages
            .into_iter()
            .filter(|(_, stage)| !stage.is_empty())
        {
            world.add_stage(stage.build());
        }
        world.run_stage(self.startup_systems.build()).unwrap();
        world.vacuum();
        world
    }

    pub fn build(mut self) -> InitializedWorlds {
        for (id, name) in self.required_plugins {
            assert!(
                self.plugins.contains(&id),
                "Plugin {name} is marked required but has not been initialized",
            );
        }

        let render_stage = self
            .stages
            .remove(&Stage::Render)
            .unwrap_or_default()
            .build();
        let mut world = self.world;
        for (_, stage) in self
            .stages
            .into_iter()
            .filter(|(_, stage)| !stage.is_empty())
        {
            world.add_stage(stage.build());
        }
        world.run_stage(self.startup_systems.build()).unwrap();
        world.vacuum();
        InitializedWorlds {
            game_world: world,
            render_stage,
        }
    }
}

pub struct InitializedWorlds {
    pub game_world: World,
    pub render_stage: SystemStage<'static>,
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
    pub(crate) next: Vec<KeyEvent>,
    pub pressed: HashSet<KeyCode>,
    pub just_released: HashSet<KeyCode>,
    pub just_pressed: HashSet<KeyCode>,
    pub events: HashMap<KeyCode, KeyEvent>,
}

impl KeyBoardInputs {
    pub fn update(&mut self) {
        self.just_released.clear();
        self.just_pressed.clear();
        self.events.clear();
        for ke in self.next.drain(..) {
            match ke.state {
                ElementState::Pressed => {
                    if let PhysicalKey::Code(k) = ke.physical_key {
                        if !self.pressed.contains(&k) {
                            self.just_pressed.insert(k);
                        }
                        self.pressed.insert(k);
                        self.events.insert(k, ke);
                    }
                }
                ElementState::Released => {
                    if let PhysicalKey::Code(k) = ke.physical_key {
                        self.pressed.remove(&k);
                        self.just_released.insert(k);
                        self.events.insert(k, ke);
                    }
                }
            }
        }
    }
}

#[derive(Default)]
pub struct MouseInputs {
    pub(crate) next: Vec<(MouseButton, ElementState)>,
    pub(crate) next_scroll: Vec<winit::event::MouseScrollDelta>,
    pub scroll: Vec<winit::event::MouseScrollDelta>,
    pub cursor_position: winit::dpi::PhysicalPosition<f64>,
    pub pressed: HashSet<MouseButton>,
    pub just_released: HashSet<MouseButton>,
    pub just_pressed: HashSet<MouseButton>,
}

impl MouseInputs {
    pub fn update(&mut self) {
        self.just_released.clear();
        self.just_pressed.clear();
        self.scroll.clear();
        std::mem::swap(&mut self.scroll, &mut self.next_scroll);
        for (k, state) in self.next.iter() {
            match state {
                ElementState::Pressed => {
                    if !self.pressed.contains(k) {
                        self.just_pressed.insert(*k);
                    }
                    self.pressed.insert(*k);
                }
                ElementState::Released => {
                    self.pressed.remove(k);
                    self.just_released.insert(*k);
                }
            }
        }
        self.next.clear();
    }
}

fn update_inputs(mut k: ResMut<KeyBoardInputs>) {
    k.update();
}

fn update_mouse_inputs(mut k: ResMut<MouseInputs>) {
    k.update();
}

pub struct InputPlugin;
impl Plugin for InputPlugin {
    fn build(self, app: &mut App) {
        app.insert_resource(KeyBoardInputs::default());
        app.insert_resource(MouseInputs::default());

        app.with_stage(Stage::PreUpdate, |s| {
            s.add_system(update_inputs).add_system(update_mouse_inputs);
        });
    }
}

pub struct TimePlugin;
impl Plugin for TimePlugin {
    fn build(self, app: &mut App) {
        app.insert_resource(Time(instant::Instant::now()));
        app.insert_resource(DeltaTime(std::time::Duration::default()));
        app.insert_resource(Tick(0));
        app.with_stage(Stage::PreUpdate, |s| {
            s.add_system(update_time);
        });
    }
}

pub struct DefaultPlugins;

impl Plugin for DefaultPlugins {
    fn build(self, app: &mut App) {
        app.add_plugin(TimePlugin);
        app.add_plugin(InputPlugin);

        app.add_plugin(TransformPlugin);
        app.add_plugin(RendererPlugin);

        app.add_plugin(UiPlugin);

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
