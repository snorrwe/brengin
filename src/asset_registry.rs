pub mod asset_loader;
mod erased_loader;

use std::{any::TypeId, collections::HashMap, path::PathBuf, sync::Arc};

use cecs::systems::SystemStageBuilder;

use crate::{asset_registry::erased_loader::ErasedLoader, oneshot::Oneshot, prelude::*};

pub struct AssetRegistryPlugin;

pub const ASSET_LOADING_STAGE: &'static str = "asset-loading-dispatch";

/// Gets or inserts the asset loading stage into `s`. The asset loading stage runs when there are
/// pending load requests
pub fn with_asset_loading_stage(app: &mut App, s: Stage, f: impl FnOnce(&mut SystemStageBuilder)) {
    let stage = app.stages.entry(s).or_default();

    if let Some(s) = stage
        .nested
        .iter_mut()
        .find(|s| s.name == ASSET_LOADING_STAGE)
    {
        f(s);
    } else {
        stage
            .add_nested_stage(SystemStage::new(ASSET_LOADING_STAGE).with_should_run(check_loading));
        f(stage.nested.last_mut().unwrap())
    }
}

impl Plugin for AssetRegistryPlugin {
    fn build(self, app: &mut App) {
        app.with_stage(Stage::PreUpdate, |s| {
            s.add_nested_stage(
                SystemStage::new(ASSET_LOADING_STAGE).with_should_run(check_loading),
            );
        });

        app.insert_resource(AssetsLoadStatus::default());
        app.insert_resource(AssetLoaders::default());
        // TODO: configure n
        app.insert_resource(AssetLoadingSemaphore(async_lock::Semaphore::new(4)));
        app.insert_resource(AssetsReceivers::default());
    }
}

fn check_loading(status: Res<AssetsLoadStatus>) -> bool {
    status.is_anything_loading()
}

#[derive(Debug, Default)]
pub struct AssetsLoadStatus(HashMap<u64, AssetLoadState>);

impl AssetsLoadStatus {
    pub fn status<T>(&self, handle: &Handle<T>) -> Option<&AssetLoadState> {
        self.0.get(&handle.id().id())
    }

    pub fn is_anything_loading(&self) -> bool {
        self.0
            .iter()
            .any(|(_, s)| matches!(s, AssetLoadState::Loading))
    }
}

#[derive(Debug, Default)]
pub enum AssetLoadState {
    Loaded,
    #[default]
    Loading,
    Error(AssetLoadError),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum AssetLoadError {
    #[error("Asset file {0} not found")]
    FileNotFound(PathBuf),
    #[error("Asset loader not found")]
    LoaderNotFound,
}

pub struct AssetLoadingSemaphore(async_lock::Semaphore);

impl AsRef<async_lock::Semaphore> for AssetLoadingSemaphore {
    fn as_ref(&self) -> &async_lock::Semaphore {
        &self.0
    }
}

pub struct AssetRegistry<'a> {
    state: ResMut<'a, AssetsLoadStatus>,
    recv: ResMut<'a, AssetsReceivers>,
    loaders: Res<'a, AssetLoaders>,
    js: Res<'a, JobPool>,
    semaphore: Res<'a, AssetLoadingSemaphore>,
}

unsafe impl<'a> WorldQuery<'a> for AssetRegistry<'a> {
    fn resources_mut(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<AssetsLoadStatus>());
        set.insert(TypeId::of::<AssetsReceivers>());
    }

    fn resources_const(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<AssetLoaders>());
        set.insert(TypeId::of::<JobPool>());
        set.insert(TypeId::of::<AssetLoadingSemaphore>());
    }

    fn new(db: &'a World, _system_idx: usize) -> Self {
        Self {
            loaders: Res::new(db),
            state: ResMut::new(db),
            recv: ResMut::new(db),
            js: Res::new(db),
            semaphore: Res::new(db),
        }
    }
}

/// Collection of ReceiverChannel<T>s for each TypeId T
///
/// Loading plugins must consume these
#[derive(Default)]
pub struct AssetsReceivers(pub HashMap<TypeId, Vec<ErasedReceiver>>);
unsafe impl Send for AssetsReceivers {}
unsafe impl Sync for AssetsReceivers {}

/// Type erased ReceiverChannel<T>
type ErasedReceiver = cecs::resources::ErasedResource;

type ReceiverChannel<T> = Arc<Oneshot<(Handle<T>, Result<T, AssetLoadError>)>>;

impl<'a> AssetRegistry<'a> {
    pub fn load<T: 'static + Send>(&mut self, path: impl Into<PathBuf>) -> Handle<T> {
        let handle = Assets::<T>::allocate();
        // TODO: gotta check if file exists and try multiple prefixes
        let future = self.loaders.load::<T>(path.into());
        let semaphore = self.semaphore.0.acquire();

        self.state
            .0
            .insert(handle.id().id(), AssetLoadState::default());

        let result_channel: ReceiverChannel<T> =
            Arc::new(Oneshot::<(Handle<T>, Result<T, AssetLoadError>)>::default());

        self.js.enqueue_future({
            let handle = handle.clone();
            let result_channel = Arc::clone(&result_channel);
            async move {
                let _permit = semaphore.await;

                let handle = handle;
                // TODO: insert result asset into Assets<T>
                // TODO: update AssetsLoadStatus
                let result = future.await;
                result_channel.send((handle, result));
            }
        });

        self.recv
            .0
            .entry(TypeId::of::<T>())
            .or_default()
            .push(ErasedReceiver::new(result_channel));

        handle
    }
}

pub trait AssetLoader<T> {
    fn load(
        &self,
        path: PathBuf,
    ) -> impl std::future::Future<Output = Result<T, AssetLoadError>> + Send;
}

#[derive(Default)]
pub struct AssetLoaders(HashMap<TypeId, Arc<ErasedLoader>>);

impl AssetLoaders {
    pub fn add_loader<T: 'static, L: AssetLoader<T> + Sync + 'static>(&mut self, loader: L) {
        self.0
            .insert(TypeId::of::<T>(), Arc::new(ErasedLoader::new(loader)));
    }

    pub fn load<T: 'static>(
        &self,
        path: PathBuf,
    ) -> impl Future<Output = Result<T, AssetLoadError>> {
        let loader = self
            .0
            .get(&TypeId::of::<T>())
            .ok_or(AssetLoadError::LoaderNotFound)
            .map(Arc::clone);

        async move { unsafe { loader?.load(path).await } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loading() {
        let mut loaders = AssetLoaders::default();

        struct NopLoader;
        impl AssetLoader<PathBuf> for NopLoader {
            fn load(
                &self,
                path: PathBuf,
            ) -> impl std::future::Future<Output = Result<PathBuf, AssetLoadError>> + Send
            {
                async move { Ok(path) }
            }
        }

        loaders.add_loader(NopLoader);

        let fut = loaders.load::<PathBuf>("test".into());

        let result = pollster::block_on(fut).expect("Load failed");

        assert_eq!(result.as_os_str(), "test");
    }

    #[test]
    fn test_unregistered_loader() {
        let loaders = AssetLoaders::default();

        let fut = loaders.load::<i64>("test".into());

        let result = pollster::block_on(fut).expect_err("Load should fail");
        assert!(matches!(result, AssetLoadError::LoaderNotFound));
    }
}
