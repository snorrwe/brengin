use std::time::Duration;

use brengin::camera::{PerspectiveCamera, WindowCamera, camera_bundle};
use brengin::prelude::*;
use brengin::renderer::sprite_renderer::{self, SpriteInstance, SpriteSheet};
use brengin::{App, DefaultPlugins};
use glam::{Vec2, Vec3};

fn animation_system(dt: Res<DeltaTime>, mut q: Query<(&mut SpriteInstance, &mut Timer)>) {
    let dt = dt.0;
    q.par_for_each_mut(move |(i, t)| {
        t.update(dt);
        if t.just_finished() {
            i.index = (i.index + 1) % 3;
        }
    });
}

fn setup(mut cmd: Commands, mut assets: ResMut<Assets<SpriteSheet>>) {
    //camera
    cmd.spawn()
        .insert(WindowCamera)
        .insert_bundle(camera_bundle(PerspectiveCamera {
            znear: 1.0,
            ..Default::default()
        }))
        .insert_bundle(transform_bundle(Transform::default()));

    let boom = load_sprite_sheet(
        include_bytes!("assets/test.png"),
        Vec2::splat(32.0),
        3,
        &mut assets,
    );

    // camera eye is in origin, span a bit in front
    cmd.spawn()
        .insert_bundle(transform_bundle(Transform::from_position(Vec3::Z * -1.5)))
        .insert_bundle(sprite_renderer::sprite_sheet_bundle(boom.clone(), None))
        .insert(Timer::new(Duration::from_secs_f32(1.0), true));
}

fn load_sprite_sheet(
    bytes: &[u8],
    box_size: Vec2,
    num_cols: u32,
    assets: &mut Assets<SpriteSheet>,
) -> Handle<SpriteSheet> {
    let image = image::load_from_memory(bytes).expect("Failed to load spritesheet");
    let sprite_sheet = SpriteSheet::from_grid(Vec2::ZERO, box_size, num_cols, image);

    assets.insert(sprite_sheet)
}

fn main() {
    tracing_subscriber::fmt::init();
    let mut app = App::default();
    app.add_plugin(DefaultPlugins);
    app.add_startup_system(setup);
    app.with_stage(brengin::Stage::Update, |s| {
        s.add_system(animation_system);
    });
    pollster::block_on(app.run()).unwrap();
}
