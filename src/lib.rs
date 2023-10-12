pub mod assets;
pub mod camera;
pub mod quat_ext;
pub mod renderer;
pub mod transform;

#[cfg(feature = "audio")]
pub mod audio;

// reexport
pub use cecs;
pub use glam;
pub use image;
use transform::TransformPlugin;
pub use winit::event::*;

use std::{any::TypeId, collections::HashSet};

use renderer::{GraphicsState, RenderResult, RendererPlugin};

use winit::{
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

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
        Self {
            world: World::new(1024),
            stages: Default::default(),
            startup_systems: SystemStage::new("startup"),
            plugins: Default::default(),
        }
    }
}

impl App {
    pub fn add_plugin<T: Plugin + 'static>(&mut self, plugin: T) {
        let id = TypeId::of::<T>();
        assert!(
            !self.plugins.contains(&id),
            "Plugins can be only registered once"
        );
        self.plugins.insert(id);
        plugin.build(self);
    }

    pub fn stage(&mut self, stage: Stage) -> &mut SystemStage<'static> {
        let stage = self
            .stages
            .entry(stage)
            .or_insert_with(move || SystemStage::new(format!("Stage-{:?}", stage)));

        // # SAFETY
        // No fucking idea, but I can't decypher the bloody compiler error so here we are
        unsafe { std::mem::transmute::<&mut SystemStage, &mut SystemStage>(stage) }
    }

    pub fn add_startup_system<P>(
        &mut self,
        sys: impl cecs::systems::IntoSystem<'static, P, ()>,
    ) -> &mut Self {
        self.startup_systems.add_system(sys);
        self
    }

    pub async fn run(mut self) {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new();

        let window = window
            .with_title("Boids") // FIXME: allow configuring the window
            .build(&event_loop)
            .expect("Failed to build window");

        #[cfg(target_family = "wasm")]
        {
            // Winit prevents sizing with CSS, so we have to set
            // the size manually when on web.
            use winit::dpi::PhysicalSize;
            window.set_inner_size(PhysicalSize::new(1960 / 2, 1080 / 2));

            use winit::platform::web::WindowExtWebSys;
            web_sys::window()
                .and_then(|win| win.document())
                .and_then(|doc| {
                    let dst = doc.body()?;
                    let canvas = web_sys::Element::from(window.canvas());
                    dst.append_child(&canvas).ok()?;
                    Some(())
                })
                .expect("Couldn't append canvas to document body.");
        }

        let graphics_state = GraphicsState::new(&window).await;

        let sprite_pipeline = renderer::sprite_renderer::SpritePipeline::new(&graphics_state);
        self.insert_resource(sprite_pipeline);
        self.insert_resource(graphics_state);

        let mut world = self.build();

        event_loop.run(move |event, _, control_flow| {
            tracing::trace!(?event, "Event received");
            match event {
                Event::WindowEvent { event, window_id } if window_id == window.id() => {
                    match event {
                        #[cfg(not(target_family = "wasm"))]
                        WindowEvent::CloseRequested
                        | WindowEvent::KeyboardInput {
                            input:
                                KeyboardInput {
                                    state: ElementState::Pressed,
                                    virtual_keycode: Some(VirtualKeyCode::Escape),
                                    ..
                                },
                            ..
                        } => *control_flow = ControlFlow::Exit,
                        WindowEvent::Resized(size) => {
                            world
                                .run_system(move |mut state: ResMut<GraphicsState>| {
                                    state.resize(size);
                                })
                                .unwrap();
                        }
                        WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                            let size = *new_inner_size;
                            world
                                .run_system(move |mut state: ResMut<GraphicsState>| {
                                    state.resize(size);
                                })
                                .unwrap();
                        }

                        WindowEvent::KeyboardInput { input, .. } => {
                            world
                                .get_resource_mut::<KeyBoardInputs>()
                                .unwrap()
                                .next
                                .push(input.clone());
                        }
                        _ => {}
                    }
                }
                Event::MainEventsCleared => {
                    // RedrawRequested will only trigger once, unless we manually
                    // request it.
                    window.request_redraw();
                }
                Event::RedrawRequested(window_id) if window_id == window.id() => {
                    // update the world
                    //
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
                                *control_flow = ControlFlow::Exit
                            }
                            // All other errors (Outdated, Timeout) should be resolved by the next frame
                            Err(e) => debug!("rendering failed: {:?}", e),
                        }
                    }
                }
                _ => {}
            }
        });
    }

    fn build(self) -> World {
        let mut world = self.world;
        for (_, stage) in self.stages {
            world.add_stage(stage);
        }
        world.run_stage(self.startup_systems).unwrap();
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
    pub inputs: Vec<KeyboardInput>,
    pub(crate) next: Vec<KeyboardInput>,
    pub pressed: HashSet<VirtualKeyCode>,
    pub just_released: HashSet<VirtualKeyCode>,
    pub just_pressed: HashSet<VirtualKeyCode>,
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
                    if let Some(k) = k.virtual_keycode {
                        if !self.pressed.contains(&k) {
                            self.just_pressed.insert(k);
                        }
                        self.pressed.insert(k);
                    }
                }
                ElementState::Released => {
                    if let Some(k) = k.virtual_keycode {
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

        app.stage(Stage::PreUpdate)
            .add_system(update_time)
            .add_system(update_inputs);

        app.add_plugin(assets::AssetsPlugin::<renderer::sprite_renderer::SpriteSheet>::default());
        app.add_plugin(TransformPlugin);
        app.add_plugin(RendererPlugin);

        #[cfg(feature = "audio")]
        app.add_plugin(audio::AudioPlugin);
    }
}
