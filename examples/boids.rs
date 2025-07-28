#![windows_subsystem = "windows"]

use brengin::{
    assets::{self, Assets, Handle},
    camera::{PerspectiveCamera, WindowCamera},
    glam::{Quat, Vec2, Vec3},
    prelude::*,
    renderer::{
        background_renderer::BackgroundImage,
        camera_bundle,
        sprite_renderer::{self, SpriteSheet},
    },
    transform::{self, transform_bundle, Transform},
    App, DefaultPlugins, DeltaTime, Plugin, Stage,
};
use image::DynamicImage;

struct Boid;

struct Velocity(pub Vec2);
struct LastVelocity(pub Vec2);

struct Pos(pub Vec2);
struct LastPos(pub Vec2);

impl std::ops::Deref for Pos {
    type Target = Vec2;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct BoidConfig {
    radius: f32,
    separation_radius: f32,
    min_vel: f32,
}

const N: usize = 1000;

fn update_boids(
    mut q: Query<(&mut Pos, &mut Velocity, &LastVelocity), With<Boid>>,
    positions: Query<&LastPos, With<Boid>>,
    conf: Res<BoidConfig>,
    dt: Res<DeltaTime>,
) {
    let radius = conf.radius;
    let sepa = conf.separation_radius;
    let min_vel = conf.min_vel;
    let dt = dt.0.as_secs_f32();
    q.par_for_each_mut(|(tr, vel, last_vel)| {
        let pos = tr.0;
        let mut dir = -min_vel * pos.normalize_or_zero(); // move towards the center if no other
                                                          // boids are in sight
        positions.iter().for_each(|gtr| {
            let d = pos - gtr.0;
            let mag = d.length();
            if mag < radius && vel.0.dot(d) < 0.0 {
                let ratio = (mag / sepa).clamp(0.01, 1.0);
                dir -= d / ratio;
            }
        });
        vel.0 = dir.lerp(last_vel.0, 0.5);
        tr.0 += vel.0 * dt;
    });
}

fn update_transform(mut q: Query<(&mut Transform, &Velocity, &Pos)>) {
    q.par_for_each_mut(|(tr, Velocity(vel), p)| {
        let angle = -vel.x.atan2(vel.y);
        tr.rot = Quat::from_rotation_z(angle);
        tr.pos = p.extend(0.0);
    });
}

fn update_boids_vel(mut q: Query<(&mut LastVelocity, &Velocity)>) {
    q.par_for_each_mut(move |(l, vel)| {
        l.0 = vel.0;
    });
}

fn update_boids_pos(mut q: Query<(&mut LastPos, &Pos)>) {
    q.par_for_each_mut(move |(l, p)| {
        l.0 = p.0;
    });
}

fn setup_background(mut cmd: Commands, mut assets: ResMut<Assets<DynamicImage>>) {
    let image = image::load_from_memory(include_bytes!("assets/boom3.png"))
        .expect("Failed to load background");
    let handle = assets.insert(image);

    cmd.insert_resource(BackgroundImage(handle));
}

fn setup_boids(mut cmd: Commands, mut assets: ResMut<assets::Assets<SpriteSheet>>) {
    //camera
    cmd.spawn()
        .insert(WindowCamera)
        .insert_bundle(camera_bundle(PerspectiveCamera {
            eye: Vec3::new(0.0, 0.0, 100.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            aspect: 16.0 / 9.0,
            fovy: std::f32::consts::TAU / 6.0,
            znear: 5.0,
            zfar: 5000.0,
        }))
        .insert_bundle(transform_bundle(transform::Transform::default()));

    let boid = load_sprite_sheet(
        include_bytes!("assets/boid.png"),
        Vec2::splat(32.0),
        1,
        &mut assets,
    );

    println!("Spawning {N} boids");
    for _ in 0..N {
        // TODO: scale by map size
        let x = fastrand::f32();
        let y = fastrand::f32();

        let vx = fastrand::f32();
        let vy = fastrand::f32();
        cmd.spawn()
            .insert_bundle(transform_bundle(transform::Transform::from_position(
                Vec3::new(x, y, 0.0),
            )))
            .insert_bundle(sprite_renderer::sprite_sheet_bundle(boid.clone(), None))
            .insert_bundle((
                Boid,
                Pos(Vec2::new(x, y)),
                LastPos(Vec2::new(x, y)),
                LastVelocity(Vec2::ZERO),
                Velocity(Vec2::new(vx, vy)),
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

struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(self, app: &mut App) {
        app.with_stage(Stage::Update, |s| {
            s.add_system(update_boids)
                .add_system(update_boids_vel.after(update_boids))
                .add_system(update_boids_pos.after(update_boids))
                .add_system(update_transform.after(update_boids));
        });

        app.add_startup_system(setup_boids);
        app.add_startup_system(setup_background);
        app.insert_resource(BoidConfig {
            radius: 30.0,
            separation_radius: 10.0,
            min_vel: 10.0,
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
