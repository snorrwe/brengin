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
use instant::Instant;
pub use wgpu;
pub use winit;

use winit::{
    application::ApplicationHandler,
    event::*,
    keyboard::{KeyCode, PhysicalKey},
    window::{Theme, WindowAttributes},
};

use std::{
    any::TypeId,
    collections::HashSet,
    panic,
    ptr::NonNull,
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};
use transform::TransformPlugin;

use renderer::{GraphicsState, RenderResult, RendererPlugin, WindowSize};

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

    extact_stage: SystemStage<'static>,
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

pub struct Window(pub Arc<winit::window::Window>);

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
            RunningApp::Pending(app) => &mut app.world,
            RunningApp::Initialized { render_world, .. } => render_world,
            RunningApp::Terminated => unreachable!(),
        }
    }
}

fn game_thread(game_world: Arc<Mutex<World>>, enabled: Arc<AtomicBool>) {
    // TODO: take from resource
    let target_frame_latency: Duration = Duration::from_millis(15);
    // reset Time so the first DT isn't outragous
    game_world
        .lock()
        .unwrap()
        .insert_resource(Time(instant::Instant::now()));
    // TODO: should_run flag to terminate the loop at shutdown
    if let Err(err) = panic::catch_unwind(|| {
        while enabled.load(std::sync::atomic::Ordering::Relaxed) {
            let start = Instant::now();

            let mut game_world = game_world.lock().unwrap();
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
    }) {
        tracing::error!(?err, "Game thread paniced!")
    }
}

impl ApplicationHandler for RunningApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Some(app) = self.as_pending() else {
            debug!("App resumed");
            return;
        };
        let attributes = app.world.get_resource_or_default::<WindowAttributes>();
        let window = event_loop
            .create_window(attributes.clone())
            .expect("Failed to create window");
        let window = Arc::new(window);
        // FIXME:
        // do not block here
        let graphics_state = pollster::block_on(GraphicsState::new(Arc::clone(&window)));

        app.render_app_mut()
            .world
            .run_system(|mut cmd: Commands| {
                cmd.spawn().insert(Window(Arc::clone(&window)));
                cmd.insert_resource(graphics_state);
            })
            .unwrap();

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

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        tracing::trace!(?event, "Event received");
        let RunningApp::Initialized {
            render_world,
            game_world,
            render_extract,
            enabled,
            ..
        } = self
        else {
            return;
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
                enabled.store(false, std::sync::atomic::Ordering::Relaxed);
                if let RunningApp::Initialized { game_thread, .. } =
                    std::mem::replace(self, RunningApp::Terminated)
                {
                    game_thread.join().expect("Failed to join game thread");
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                let w = Arc::clone(game_world);
                render_world
                    .run_system(move |mut state: ResMut<GraphicsState>| {
                        let mut w = w.lock().unwrap();
                        w.insert_resource(WindowSize {
                            width: size.width,
                            height: size.height,
                        });

                        state.resize(size);
                    })
                    .unwrap();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                game_world
                    .lock()
                    .unwrap()
                    .get_resource_mut::<KeyBoardInputs>()
                    .unwrap()
                    .next
                    .push(event.clone());
            }
            WindowEvent::RedrawRequested => {
                {
                    // extraction
                    let mut gw = game_world.lock().unwrap();
                    render_world.insert_resource(GameWorld {
                        world: NonNull::from(&mut *gw),
                    });
                    render_world
                        .run_system(|mut cmd: Commands, tick: Option<ResMut<ExtractionTick>>| {
                            match tick {
                                Some(mut t) => t.0 += 1,
                                None => cmd.insert_resource(ExtractionTick(0)),
                            }
                        })
                        .unwrap();
                    render_world.run_stage(render_extract.clone()).unwrap();
                    render_world.remove_resource::<GameWorld>();
                }

                render_world.tick();

                let result = render_world.get_resource::<RenderResult>();
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
        if let RunningApp::Terminated = self {
            return;
        }
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
            world.add_stage(stage);
        }
        world.run_stage(self.startup_systems).unwrap();
        world.vacuum();
        world
    }

    fn build(mut self) -> InitializedWorlds {
        let rw = self
            .render_app
            .take()
            .map(|a| a._build())
            .unwrap_or_else(|| World::new(4));
        let render_extract = std::mem::replace(&mut self.extact_stage, SystemStage::new("nil"));
        let w = self._build();
        InitializedWorlds {
            game_world: w,
            render_world: rw,
            render_extract,
        }
    }
}

struct InitializedWorlds {
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

pub struct InputPlugin;
impl Plugin for InputPlugin {
    fn build(self, app: &mut App) {
        app.insert_resource(KeyBoardInputs::default());

        app.with_stage(Stage::PreUpdate, |s| {
            s.add_system(update_time).add_system(update_inputs);
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
