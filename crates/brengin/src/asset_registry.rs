pub mod asset_loader;
mod erased_loader;
pub mod font_loader;
pub mod image_loader;
pub mod sprite_sheet_loader;

#[cfg(feature = "tar")]
pub mod tar_loader;

use crate::HashMap;

use std::{
    any::TypeId, borrow::Cow, io::Read as _, path::PathBuf, sync::Arc,
    thread::available_parallelism,
};

use cecs::systems::SystemStageBuilder;

use crate::{asset_registry::erased_loader::ErasedLoader, oneshot::Oneshot, prelude::*};

pub struct AssetRegistryPlugin;

pub const ASSET_LOADING_STAGE: &str = "asset-loading-dispatch";

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

        app.get_or_insert_resource(FileLoaders::default);
        app.insert_resource(AssetsLoadStatus::default());
        app.insert_resource(AssetLoaders::default());
        app.get_or_insert_resource(|| {
            let n = available_parallelism()
                .map(|x| x.get() * 2 / 3)
                .unwrap_or(1)
                .max(1);
            AssetLoadingSemaphore(async_lock::Semaphore::new(n))
        });
        app.insert_resource(AssetsReceivers::default());

        // add bundled loaders
        app.add_plugin(asset_loader::AssetLoaderPlugin::new(
            sprite_sheet_loader::SpriteSheetLoader,
        ));
        app.add_plugin(asset_loader::AssetLoaderPlugin::new(
            image_loader::DynamicImageLoader,
        ));
        app.add_plugin(asset_loader::AssetLoaderPlugin::new(
            font_loader::FontLoader,
        ));
    }
}

/// File loaders that the AssetRegistry will try to use to load the requested assets
/// You can override these by either inserting an FileAssetLoaders resource before the
/// AssetRegistryPlugin or you can override the resource, but it will only take effect for loads
/// _after_ the modification.
///
/// It is recommended that you override before the plugin load
pub struct FileLoaders(pub Vec<Arc<dyn FileLoader>>);

impl FileLoaders {
    pub fn join(mut self, rhs: Self) -> Self {
        self.0.extend(rhs.0);
        self
    }

    pub fn with(mut self, loader: impl FileLoader + 'static) -> Self {
        self.0.push(Arc::new(loader));
        self
    }

    pub fn default_dir_loaders() -> FileLoaders {
        let mut default_paths = Vec::<Arc<dyn FileLoader>>::new();
        if let Some(v) =
            std::env::var_os("CARGO_MANIFEST_DIR").map(|s| PathBuf::from(s).join(ASSETS_PATH))
            && v.is_dir()
        {
            default_paths.push(FsFileLoader::from_path(v));
        }
        if let Some(v) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join(ASSETS_PATH)))
            && v.is_dir()
        {
            default_paths.push(FsFileLoader::from_path(v));
        }
        if let Ok(v) = std::env::current_dir().map(|p| p.join(ASSETS_PATH))
            && v.is_dir()
        {
            default_paths.push(FsFileLoader::from_path(v));
        }

        Self(default_paths)
    }

    // TODO: insert asset pack loaders here
}

const ASSETS_PATH: &str = "assets";

impl Default for FileLoaders {
    fn default() -> Self {
        Self::default_dir_loaders()
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
    #[error("File load failed: {0}")]
    LoadFailed(String),
}

pub struct AssetLoadingSemaphore(async_lock::Semaphore);

impl AsRef<async_lock::Semaphore> for AssetLoadingSemaphore {
    fn as_ref(&self) -> &async_lock::Semaphore {
        &self.0
    }
}

cecs::query_collection! {
    pub struct AssetRegistry<'a> {
        basepaths: Res<'a, FileLoaders>,
        state: ResMut<'a, AssetsLoadStatus>,
        recv: ResMut<'a, AssetsReceivers>,
        loaders: Res<'a, AssetLoaders>,
        js: Res<'a, JobPool>,
        semaphore: Res<'a, AssetLoadingSemaphore>,
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

async fn try_load_asset<T, F>(
    semaphore: async_lock::futures::Acquire<'_>,
    handle: Handle<T>,
    result_channel: ReceiverChannel<T>,
    futures: Vec<(PathBuf, F)>,
    path: PathBuf,
) where
    F: Future<Output = Result<T, AssetLoadError>>,
{
    let _permit = semaphore.await;
    let handle = handle;
    for (_path, future) in futures {
        let result = future.await;

        #[cfg(feature = "tracing")]
        match result.as_ref() {
            Ok(_) | Err(AssetLoadError::FileNotFound(_)) => {
                tracing::debug!(result=?result.as_ref().map(drop), path=_path.to_str(), "Load result");
            }
            Err(err) => {
                tracing::error!(?err, path = _path.to_str(), "Load failed");
            }
        }

        // return the first success, or not-notfound error
        if result.is_ok() || !matches!(result, Err(AssetLoadError::FileNotFound(_))) {
            result_channel.send((handle, result));
            return;
        }
    }
    result_channel.send((handle, Err(AssetLoadError::FileNotFound(path))));
}

impl<'a> AssetRegistry<'a> {
    fn iter_file_loaders<'b>(&'b self) -> impl Iterator<Item = Arc<dyn FileLoader>> + 'b {
        self.basepaths.0.iter().cloned()
    }

    /// waits for the asset load to complete before proceeding
    pub fn load_sync<T: 'static + Send>(&mut self, path: impl AsRef<std::path::Path>) -> Handle<T> {
        // TODO: only the future handling is different, could probably dedupe more

        let path = path.as_ref().to_path_buf();

        let handle = Assets::<T>::allocate();
        let futures = self
            .iter_file_loaders()
            .map(|loader| (path.clone(), self.loaders.load::<T>(path.clone(), loader)))
            .collect::<Vec<_>>();
        self.state
            .0
            .insert(handle.id().id(), AssetLoadState::default());

        let result_channel: ReceiverChannel<T> =
            Arc::new(Oneshot::<(Handle<T>, Result<T, AssetLoadError>)>::default());

        pollster::block_on(try_load_asset(
            self.semaphore.0.acquire(),
            handle.clone(),
            Arc::clone(&result_channel),
            futures,
            path,
        ));

        self.recv
            .0
            .entry(TypeId::of::<T>())
            .or_default()
            .push(ErasedReceiver::new(result_channel));

        handle
    }

    pub fn load<T: 'static + Send>(&mut self, path: impl AsRef<std::path::Path>) -> Handle<T> {
        let path = path.as_ref().to_path_buf();
        let handle = Assets::<T>::allocate();
        let futures = self
            .iter_file_loaders()
            .map(|loader| (path.clone(), self.loaders.load::<T>(path.clone(), loader)))
            .collect::<Vec<_>>();
        self.state
            .0
            .insert(handle.id().id(), AssetLoadState::default());

        let result_channel: ReceiverChannel<T> =
            Arc::new(Oneshot::<(Handle<T>, Result<T, AssetLoadError>)>::default());

        self.js.enqueue_future(try_load_asset(
            self.semaphore.0.acquire(),
            handle.clone(),
            Arc::clone(&result_channel),
            futures,
            path,
        ));

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
        loader: Arc<dyn FileLoader>,
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
        file_loader: Arc<dyn FileLoader>,
    ) -> impl Future<Output = Result<T, AssetLoadError>> {
        let loader = self
            .0
            .get(&TypeId::of::<T>())
            .ok_or(AssetLoadError::LoaderNotFound)
            .map(Arc::clone);

        async move { unsafe { loader?.load(path, file_loader).await } }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{0} not found")]
    NotFound(std::path::PathBuf),
    #[error("{0} could not be opened")]
    OpenFailed(std::path::PathBuf),
    #[error("Failed to load {0}")]
    LoadFailed(std::path::PathBuf),
}

pub trait FileLoader: Sync + Send {
    /// Load the entire file into memory
    fn load_all<'a>(&'a self, p: &std::path::Path) -> Result<Cow<'a, [u8]>, LoadError>;
}

pub struct FsFileLoader {
    pub base_path: PathBuf,
}

impl FsFileLoader {
    pub fn from_path(p: impl Into<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            base_path: p.into(),
        })
    }
}

impl FileLoader for FsFileLoader {
    fn load_all<'a>(&'a self, p: &std::path::Path) -> Result<Cow<'a, [u8]>, LoadError> {
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .open(self.base_path.join(p))
            .map_err(|_err| LoadError::OpenFailed(p.to_path_buf()))?;

        let mut result = Vec::new();
        f.read_to_end(&mut result)
            .map_err(|_err| LoadError::LoadFailed(p.to_path_buf()))?;

        Ok(std::borrow::Cow::Owned(result))
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
            async fn load(
                &self,
                path: PathBuf,
                _loader: Arc<dyn FileLoader>,
            ) -> Result<PathBuf, AssetLoadError> {
                Ok(path)
            }
        }

        loaders.add_loader(NopLoader);

        let fut = loaders.load::<PathBuf>("test".into(), FsFileLoader::from_path("/"));

        let result = pollster::block_on(fut).expect("Load failed");

        assert_eq!(result.as_os_str(), "test");
    }

    #[test]
    fn test_unregistered_loader() {
        let loaders = AssetLoaders::default();

        let fut = loaders.load::<i64>(
            "test".into(),
            Arc::new(FsFileLoader {
                base_path: "/".into(),
            }),
        );

        let result = pollster::block_on(fut).expect_err("Load should fail");
        assert!(matches!(result, AssetLoadError::LoaderNotFound));
    }
}
