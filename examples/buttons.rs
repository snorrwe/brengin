use brengin::prelude::*;
use brengin::ui::builder::Ui;
use brengin::{App, DefaultPlugins};
use tracing::info;

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

async fn game() {
    let mut app = App::default();
    app.add_plugin(DefaultPlugins);
    app.with_stage(brengin::Stage::Update, |s| {
        s.add_system(buttons_ui);
    });
    app.run().await.unwrap();
}

fn main() {
    tracing_subscriber::fmt::init();
    pollster::block_on(game());
}
