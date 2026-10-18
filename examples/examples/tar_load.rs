use std::{process::exit, sync::Arc};

use brengin::{
    asset_registry::{
        AssetLoadState, AssetRegistry, AssetsLoadStatus, FileLoaders, tar_loader::TarLoader,
    },
    prelude::*,
    renderer::sprite_renderer::SpriteSheet,
};

struct Assets {
    boom: Handle<SpriteSheet>,
}

fn setup(mut reg: AssetRegistry, mut cmd: Commands) {
    let boom = reg.load("boom3.json");
    cmd.insert_resource(Assets { boom });
}

fn check(assets: Res<Assets>, status: Res<AssetsLoadStatus>) {
    let status = status.status(&assets.boom).unwrap();
    match status {
        AssetLoadState::Loaded => {
            println!("Successful load");
            exit(0);
        }
        AssetLoadState::Error(err) => {
            println!("Failed to load {err:?}");
            exit(1);
        }
        AssetLoadState::Loading => {}
    }
}

async fn game() {
    let mut app = App::default();

    let loader =
        TarLoader::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets.data"))
            .unwrap();
    loader
        .assert_checksum(include_str!("../assets.checksum"))
        .unwrap();

    // must insert before the AssetRegistryPlugin
    app.insert_resource(FileLoaders(vec![Arc::new(loader)]));

    app.add_plugin(DefaultPlugins);

    app.add_startup_system(setup);
    app.with_stage(Stage::Update, |s| {
        s.add_system(check);
    });

    app.run().await.unwrap();
}

fn main() {
    tracing_subscriber::fmt::init();
    pollster::block_on(game());
}
