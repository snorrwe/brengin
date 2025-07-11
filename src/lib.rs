pub mod assets;
pub mod camera;
pub mod prelude;
pub mod quat_ext;
pub mod renderer;
pub mod transform;

#[cfg(feature = "audio")]
pub mod audio;
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
    any::TypeId,
    collections::{HashMap, HashSet},
    ptr::NonNull,
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
    repeat: bool,
    just_finished: bool,
}

pub struct GameWorld {
    world: NonNull<World>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionTick(pub u32);

unsafe impl Send for GameWorld {}
unsafe impl Sync for GameWorld {}

impl GameWorld {
    pub fn world(&self) -> &World {
        unsafe { self.world.as_ref() }
    }

    pub fn world_mut(&mut self) -> &mut World {
        unsafe { self.world.as_mut() }
    }
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

/// extraction
pub fn extract_render_data(
    game_world: &Mutex<World>,
    render_world: &mut World,
    render_extract: &SystemStage,
) {
    let Some(mut gw) = game_world.try_lock_for(Duration::from_millis(6)) else {
        #[cfg(feature = "tracing")]
        tracing::debug!("game_world lock failed. rendering the previous frame");
        return;
    };
    render_world
        .run_system(|mut tick: ResMut<ExtractionTick>| {
            tick.0 += 1;
        })
        .unwrap();
    render_world.insert_resource(GameWorld {
        world: NonNull::from(&mut *gw),
    });
    render_world
        .run_system(
            |world: Res<GameWorld>, mut cmd: Commands, q: Query<EntityId>| {
                // clear invalid ids
                for id in q.iter() {
                    if !world.world().is_id_valid(id) {
                        cmd.delete(id);
                    }
                }
            },
        )
        .unwrap();
    render_world.run_stage(render_extract.clone()).unwrap();
    render_world.remove_resource::<GameWorld>();
}

pub struct App {
    world: World,
    stages: std::collections::BTreeMap<Stage, SystemStageBuilder<'static>>,
    startup_systems: SystemStageBuilder<'static>,
    plugins: HashSet<TypeId>,

    extact_stage: SystemStageBuilder<'static>,
    pub render_app: Option<Box<App>>,
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
        let mut app = App::empty();
        app.render_app = Some(Box::new(App::empty()));
        app
    }
}

enum RunningApp {
    Pending(App),
    Initialized {
        render_world: World,
        render_extract: SystemStage<'static>,
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

    fn world_mut(&mut self) -> &mut World {
        match self {
            RunningApp::Pending(app) => &mut app.render_app_mut().world,
            RunningApp::Initialized { render_world, .. } => render_world,
            RunningApp::Terminated => unreachable!(),
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
        app.render_app_mut().insert_resource(graphics_state);

        let InitializedWorlds {
            game_world,
            render_world,
            render_extract,
        } = std::mem::take(app).build();
        let game_world = Arc::new(Mutex::new(game_world));
        let enabled = Arc::new(AtomicBool::new(true));
        let game_thread = std::thread::spawn({
            let game_world = Arc::clone(&game_world);
            let enabled = Arc::clone(&enabled);
            move || game_thread(game_world, enabled)
        });
        *self = RunningApp::Initialized {
            render_world,
            game_world,
            game_thread,
            render_extract,
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

        app.render_app_mut().insert_resource(window);

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
            render_world,
            game_world,
            render_extract,
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
                let w = Arc::clone(game_world);
                render_world
                    .run_system(move |mut state: ResMut<GraphicsState>| {
                        let mut w = w.lock();
                        w.insert_resource(WindowSize {
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
                extract_render_data(&game_world, render_world, render_extract);

                render_world.tick();

                let result = render_world.get_resource::<RenderResult>();
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
        self.world_mut()
            .run_system(|window: Res<Arc<winit::window::Window>>| {
                #[cfg(feature = "tracing")]
                tracing::trace!("redraw {:?}", &*window);
                window.request_redraw();
            })
            .unwrap();
        #[cfg(feature = "tracing")]
        tracing::trace!("✓ about_to_wait");
    }
}

impl App {
    pub fn render_app(&self) -> &App {
        self.render_app.as_ref().unwrap()
    }

    pub fn render_app_mut(&mut self) -> &mut App {
        self.render_app.as_mut().unwrap()
    }

    fn empty() -> Self {
        let mut world = World::new(1024);
        world.insert_resource(WindowDescriptor::default());
        Self {
            world,
            stages: Default::default(),
            startup_systems: SystemStage::new("startup"),
            extact_stage: SystemStage::new("extract"),
            plugins: Default::default(),
            render_app: None,
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

    pub fn add_startup_system<P>(
        &mut self,
        sys: impl cecs::systems::IntoSystem<'static, P, ()>,
    ) -> &mut Self {
        self.startup_systems.add_system(sys);
        self
    }

    /// extraction systems run on the RenderWorld with readonly access to the GameWorld
    pub fn add_extract_system<P>(
        &mut self,
        sys: impl cecs::systems::IntoSystem<'static, P, ()>,
    ) -> &mut Self {
        self.extact_stage.add_system(sys);
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
        if self
            .stages
            .get(&Stage::Render)
            .map(|s| s.is_empty())
            .unwrap_or(false)
        {
            #[cfg(feature = "tracing")]
            tracing::warn!("Rendering is performed in a sub-app not the main app. But the main app's Render stage is non-empty.");
        }
        let rw = self
            .render_app
            .take()
            .map(|a| a._build())
            .unwrap_or_else(|| World::new(4));
        let render_extract =
            std::mem::replace(&mut self.extact_stage, SystemStage::new("nil")).build();
        let w = self._build();
        InitializedWorlds {
            game_world: w,
            render_world: rw,
            render_extract,
        }
    }
}

pub struct InitializedWorlds {
    pub game_world: World,
    pub render_world: World,
    pub render_extract: SystemStage<'static>,
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
            s.add_system(update_time)
                .add_system(update_inputs)
                .add_system(update_mouse_inputs);
        });
    }
}

pub struct TimePlugin;
impl Plugin for TimePlugin {
    fn build(self, app: &mut App) {
        app.insert_resource(Time(instant::Instant::now()));
        app.insert_resource(DeltaTime(std::time::Duration::default()));
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
