use std::{any::TypeId, collections::HashMap, mem::MaybeUninit, path::PathBuf, pin::Pin};

use crate::prelude::*;

pub struct AssetRegistryPlugin;

pub const ASSET_LOADING_PRE_STAGE: &'static str = "asset-loading-dispatch";

impl Plugin for AssetRegistryPlugin {
    fn build(self, app: &mut App) {
        app.with_stage(Stage::PreUpdate, |s| {
            s.add_nested_stage(
                SystemStage::new(ASSET_LOADING_PRE_STAGE).with_should_run(check_loading),
            );
        });

        app.insert_resource(AssetsLoadStatus::default());

        todo!()
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

pub struct AssetRegistry {}

pub trait AssetLoader<T> {
    fn load(
        &self,
        path: PathBuf,
    ) -> impl std::future::Future<Output = Result<T, AssetLoadError>> + Send;
}

#[derive(Default)]
pub struct AssetLoaders(HashMap<TypeId, ErasedLoader>);

impl AssetLoaders {
    pub fn add_loader<T: 'static, L: AssetLoader<T> + Sync + 'static>(&mut self, loader: L) {
        self.0.insert(TypeId::of::<T>(), ErasedLoader::new(loader));
    }

    pub async fn load<T: 'static>(&self, path: impl Into<PathBuf>) -> Result<T, AssetLoadError> {
        let loader = self
            .0
            .get(&TypeId::of::<T>())
            .ok_or(AssetLoadError::LoaderNotFound)?;

        unsafe { loader.load(path).await }
    }
}

/// Type erased asset loader
struct ErasedLoader {
    inner: *mut u8, // type L
    finalize: fn(&mut ErasedLoader),
    /// Cumbersome workaround the fact that type L is not known when load is called
    load: fn(
        &ErasedLoader,
        path: PathBuf,
        output: *mut u8, // must be type T
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), AssetLoadError>> + Send>>,
}

impl Drop for ErasedLoader {
    fn drop(&mut self) {
        (self.finalize)(self);
    }
}

unsafe impl Sync for ErasedLoader {}

#[expect(dead_code)]
impl ErasedLoader {
    pub fn new<L, T>(value: L) -> Self
    where
        L: AssetLoader<T> + Sync + 'static,
        T: 'static,
    {
        let inner = Box::leak(Box::new(value));
        Self {
            inner: (inner as *mut L).cast(),
            finalize: |resource| unsafe {
                if !resource.inner.is_null() {
                    let _inner: Box<L> = Box::from_raw(resource.inner.cast::<L>());
                    resource.inner = std::ptr::null_mut();
                }
            },
            load: |resource, path, out: *mut u8| unsafe {
                assert!(!resource.inner.is_null());
                let _inner: &L = &*resource.inner.cast::<L>();
                struct Out<T>(*mut T);
                unsafe impl<T> Send for Out<T> {}
                let out = Out(out.cast::<T>());
                Box::pin(async move {
                    // move otherwise only out.0 is moved into the async
                    // block which is not Send
                    let out = out;
                    let result = _inner.load(path).await?;
                    std::ptr::write(out.0, result);
                    Ok(())
                })
            },
        }
    }

    /// # SAFETY
    /// Must be called with the same type as `new`
    pub async unsafe fn load<T>(&self, path: impl Into<PathBuf>) -> Result<T, AssetLoadError> {
        let mut result = MaybeUninit::<T>::uninit();
        (self.load)(self, path.into(), result.as_mut_ptr().cast())
            .await
            .map(move |_| unsafe { result.assume_init() })
    }

    /// # SAFETY
    /// Must be called with the same type as `new`
    pub unsafe fn as_inner<T>(&self) -> &T {
        unsafe { &*self.inner.cast() }
    }

    /// # SAFETY
    /// Must be called with the same type as `new`
    pub unsafe fn as_inner_mut<T>(&mut self) -> &mut T {
        unsafe { &mut *self.inner.cast() }
    }

    pub unsafe fn into_inner<T>(mut self) -> Box<T> {
        unsafe {
            let inner = self.inner;
            self.inner = std::ptr::null_mut();
            Box::from_raw(inner.cast())
        }
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

        let fut = loaders.load::<PathBuf>("test");

        let result = pollster::block_on(fut).expect("Load failed");

        assert_eq!(result.as_os_str(), "test");
    }

    #[test]
    fn test_unregistered_loader() {
        let loaders = AssetLoaders::default();

        let fut = loaders.load::<i64>("test");

        let result = pollster::block_on(fut).expect_err("Load should fail");
        assert!(matches!(result, AssetLoadError::LoaderNotFound));
    }
}
