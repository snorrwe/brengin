use brengin::camera::{camera_bundle, PerspectiveCamera, WindowCamera};
use brengin::ui::{HorizontalAlignment, Ui, UiCoordinate, VerticalAlignment};
use brengin::{prelude::*, transform};
use brengin::{App, DefaultPlugins};
use glam::Vec3;

struct Label(String);

fn buttons_ui(mut ctx: Ui, mut label: ResMut<Label>) {
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoordinate::Percent(100),
            height: 300.into(),
            horizonal: HorizontalAlignment::Right,
            vertical: VerticalAlignment::Bottom,
        },
        |ui| {
            ui.grid(5, |cols| {
                for col in 0..4 {
                    cols.column(col, |ui| {
                        for row in 0..4 {
                            ui.horizontal(|ui| {
                                ui.label("Click this one pls");
                                let fill = row * 2;
                                let l = format!("{row} {col}\nPoggies{:s>fill$}", "");
                                if ui.button(&l).pressed() {
                                    label.0 = l;
                                }
                            });
                        }
                    });
                }
            });
        },
    );
    ctx.panel(
        brengin::ui::PanelDescriptor {
            width: UiCoordinate::Percent(50),
            height: 200.into(),
            horizonal: HorizontalAlignment::Center,
            vertical: VerticalAlignment::Center,
        },
        |ui| {
            ui.with_theme(
                brengin::ui::Theme {
                    font_size: 24,
                    ..ui.theme().clone()
                },
                |ui| {
                    ui.label("My panel is centered!!");
                },
            );
            ui.horizontal(|ui| {
                ui.label("Selected: ");
                ui.label(&label.0);
            });
        },
    );
}

fn setup(mut cmd: Commands) {
    //camera
    cmd.spawn()
        .insert(WindowCamera)
        .insert_bundle(camera_bundle(PerspectiveCamera::default()))
        .insert_bundle(transform_bundle(transform::Transform::default()));
}

async fn game() {
    let mut app = App::default();
    app.insert_resource(Label(Default::default()));
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
