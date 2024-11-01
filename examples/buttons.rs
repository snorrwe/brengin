use brengin::camera::{camera_bundle, PerspectiveCamera, WindowCamera};
use brengin::ui::Ui;
use brengin::{prelude::*, transform};
use brengin::{App, DefaultPlugins};
use glam::Vec3;
use tracing::info;

fn buttons_ui(mut ctx: Ui) {
    ctx.grid(4, |mut cols| {
        for col in 0..4 {
            cols.column(col, |ui| {
                for row in 0..4 {
                    let fill = row * 2;
                    let label = format!("{row} {col} Poggies {:0>fill$}", "");
                    ui.label("some label");
                    if ui.button(label).pressed {
                        info!("Poggies {row} {col}")
                    }
                }
            });
        }
    });
}

fn setup(mut cmd: Commands) {
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
}

async fn game() {
    let mut app = App::default();
    app.add_plugin(DefaultPlugins);
    app.add_startup_system(setup);
    app.with_stage(brengin::Stage::Update, |s| {
        s.add_system(buttons_ui);
    });
    app.run().await.unwrap();
}

fn main() {
    tracing_subscriber::fmt::init();
    pollster::block_on(game());
}
