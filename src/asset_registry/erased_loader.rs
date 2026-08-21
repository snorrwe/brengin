use std::{ffi::c_void, mem::MaybeUninit, path::PathBuf, pin::Pin};

use super::{AssetLoadError, AssetLoader};

/// Type erased asset loader
pub struct ErasedLoader {
    inner: *mut c_void, // type L
    finalize: fn(&mut ErasedLoader),
    /// Cumbersome workaround the fact that type L is not known when load is called
    load: fn(
        &ErasedLoader,
        path: PathBuf,
        output: *mut c_void, // must be type T
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), AssetLoadError>> + Send>>,
}

impl Drop for ErasedLoader {
    fn drop(&mut self) {
        (self.finalize)(self);
    }
}

unsafe impl Send for ErasedLoader {}
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
            load: |resource, path, out: *mut c_void| unsafe {
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
