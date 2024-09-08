use brengin::prelude::*;
use brengin::ui::builder::Ui;
use brengin::{App, DefaultPlugins, Plugin};
use tracing::info;

struct GamePlugin;

fn buttons_ui(mut ctx: ResMut<Ui>) {
    ctx.grid(4, |mut cols| {
        for col in 0..4 {
            cols.column(col, |ui| {
                for row in 0..4 {
                    if ui.button(format!("Button row: {row} col: {col}")).pressed {
                        info!("Poggies {row} {col}")
                    }
                }
            });
        }
    });
}

impl Plugin for GamePlugin {
    fn build(self, app: &mut brengin::App) {
        app.with_stage(brengin::Stage::Update, |s| {
            s.add_system(buttons_ui);
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
