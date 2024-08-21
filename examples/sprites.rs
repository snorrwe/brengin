use std::time::Duration;

use brengin::camera::{camera_bundle, Camera3d, WindowCamera};
use brengin::prelude::*;
use brengin::renderer::sprite_renderer::{self, SpriteInstance, SpriteSheet};
use brengin::{App, DefaultPlugins, Plugin};
use glam::{Quat, Vec2, Vec3};

struct GamePlugin;

const N: usize = 10000;

fn animation_system(dt: Res<DeltaTime>, mut q: Query<(&mut SpriteInstance, &mut Timer)>) {
    let dt = dt.0;
    q.par_for_each_mut(move |(i, t)| {
        t.update(dt);
        if t.just_finished() {
            i.index = (i.index + 1) % 64;
        }
    });
}

fn camera_rotation_system(dt: Res<DeltaTime>, mut q: Query<&mut Transform, With<WindowCamera>>) {
    for tr in q.iter_mut() {
        tr.rot = tr
            .rot
            .mul_quat(Quat::from_rotation_y(
                std::f32::consts::TAU / 8.0 * dt.0.as_secs_f32(),
            ))
            .normalize();
        if !tr.rot.is_normalized() {
            tr.rot = Quat::default();
        }
    }
}

fn setup(mut cmd: Commands, mut assets: ResMut<Assets<SpriteSheet>>) {
    //camera
    cmd.spawn()
        .insert(WindowCamera)
        .insert_bundle(camera_bundle(Camera3d {
            eye: Vec3::new(0.0, 0.0, 50.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            aspect: 16.0 / 9.0,
            fovy: std::f32::consts::TAU / 6.0,
            znear: 5.0,
            zfar: 5000.0,
        }))
        .insert_bundle(transform_bundle(Transform::default()));

    let boom = load_sprite_sheet(
        include_bytes!("assets/boom3.png"),
        Vec2::splat(128.0),
        8,
        &mut assets,
    );

    for _ in 0..N {
        let x = fastrand::f32() * 50.0 - 25.0;
        let y = fastrand::f32() * 50.0 - 25.0;
        let z = fastrand::f32() * 50.0 - 25.0;

        cmd.spawn()
            .insert_bundle(transform_bundle(Transform::from_position(Vec3::new(
                x, y, z,
            ))))
            .insert_bundle(sprite_renderer::sprite_sheet_bundle(boom.clone(), None))
            .insert(Timer::new(
                Duration::from_secs_f32(fastrand::f32() / 30.0),
                true,
            ));
    }
}

fn load_sprite_sheet(
    bytes: &[u8],
    box_size: Vec2,
    num_cols: u32,
    assets: &mut Assets<SpriteSheet>,
) -> Handle<SpriteSheet> {
    let image = image::load_from_memory(bytes).expect("Failed to load spritesheet");
    let sprite_sheet = SpriteSheet::from_image(Vec2::ZERO, box_size, num_cols, image);

    assets.insert(sprite_sheet)
}

impl Plugin for GamePlugin {
    fn build(self, app: &mut brengin::App) {
        app.add_startup_system(setup);
        app.with_stage(brengin::Stage::Update, |s| {
            s.add_system(animation_system)
                .add_system(camera_rotation_system);
        });
    }
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
