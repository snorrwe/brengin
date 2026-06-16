use std::{
    any::{TypeId, type_name},
    collections::BTreeMap,
    marker::PhantomData,
    sync::atomic::Ordering,
};

use cecs::Component;

use crate::prelude::*;

pub struct AssetStatsEntry {
    pub data_references: usize,
    pub weak_references: usize,
    pub type_name: String,
    pub size: usize,
}

fn setup_asset_stats_entry<T: 'static>(mut stats: ResMut<AssetStats>) {
    let t = TypeId::of::<T>();
    stats.references.insert(
        t,
        AssetStatsEntry {
            data_references: 0,
            weak_references: 0,
            type_name: type_name::<T>().to_owned(),
            size: 0,
        },
    );
}

fn update_stats_system<T: 'static>(assets: Res<Assets<T>>, mut stats: ResMut<AssetStats>) {
    let t = TypeId::of::<T>();

    let entry = stats.references.get_mut(&t).unwrap();
    entry.size = assets.assets.len();

    let mut data = 0;
    let mut weak = 0;
    assets.assets.iter().for_each(|(_id, val)| {
        let d = val.handle.data();
        data += d.data_references.load(Ordering::Relaxed);
        weak += d.weak_references.load(Ordering::Relaxed);
    });
}

#[derive(Default)]
pub struct AssetStats {
    pub references: BTreeMap<TypeId, AssetStatsEntry>,
}

pub struct AssetStatsPlugin<T> {
    _m: PhantomData<T>,
}

impl<T> Default for AssetStatsPlugin<T> {
    fn default() -> Self {
        Self { _m: PhantomData }
    }
}

impl<T: Component> Plugin for AssetStatsPlugin<T> {
    fn build(self, app: &mut App) {
        app.get_or_insert_resource(AssetStats::default);
        app.add_startup_system(setup_asset_stats_entry::<T>);

        app.with_stage(crate::Stage::PostUpdate, |s| {
            s.add_system(update_stats_system::<T>.after(super::gc_assets::<T>));
        });
    }
}
