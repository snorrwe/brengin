use cecs::Component;

use crate::{
    asset_registry::{
        AssetLoadState, AssetLoaders, AssetsLoadStatus, AssetsReceivers, ReceiverChannel,
        with_asset_loading_stage,
    },
    prelude::*,
};
use std::{any::TypeId, marker::PhantomData};

pub struct AssetLoaderPlugin<T, L>
where
    L: super::AssetLoader<T>,
{
    loader: L,
    _m: PhantomData<T>,
}

impl<T, L> AssetLoaderPlugin<T, L>
where
    L: super::AssetLoader<T>,
{
    pub fn new(loader: L) -> Self {
        Self {
            loader,
            _m: PhantomData,
        }
    }
}

impl<T: Component, L> Plugin for AssetLoaderPlugin<T, L>
where
    L: super::AssetLoader<T> + Sync + 'static,
{
    fn build(self, app: &mut App) {
        app.require_plugin(AssetsPlugin::<T>::default());

        let loaders = app.get_or_insert_resource(AssetLoaders::default);
        loaders.add_loader(self.loader);

        with_asset_loading_stage(app, Stage::PreUpdate, |s| {
            s.add_system(handle_loads::<T>);
        });
    }
}

fn handle_loads<T: 'static>(
    mut status: ResMut<AssetsLoadStatus>,
    mut recv: ResMut<AssetsReceivers>,
    mut assets: ResMut<Assets<T>>,
) {
    let Some(v) = recv.0.get_mut(&TypeId::of::<T>()) else {
        return;
    };

    for i in (0..v.len()).rev() {
        let recv = &mut v[i];
        unsafe {
            let recv: &mut ReceiverChannel<T> = recv.as_inner_mut();
            if let Some((id, result)) = recv.try_receive() {
                v.swap_remove(i);

                let s = match result {
                    Ok(value) => {
                        assets.set(&id, value);
                        AssetLoadState::Loaded
                    }
                    Err(err) => AssetLoadState::Error(err),
                };

                status.0.insert(id.id().id(), s);
            }
        }
    }
}
