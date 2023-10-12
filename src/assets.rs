use cecs::{prelude::*, Component};
use std::{
    collections::HashMap,
    marker::PhantomData,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};
use tracing::debug;

use crate::Plugin;

pub type AssetId = u64;

struct RefCount {
    data_references: AtomicUsize,
    weak_references: AtomicUsize,
}

pub struct Handle<T> {
    id: AssetId,
    weak: WeakHandle<T>,
}

impl<T> Default for Handle<T> {
    fn default() -> Self {
        Self::new(AssetId::MAX)
    }
}

pub struct WeakHandle<T> {
    id: AssetId,
    references: NonNull<RefCount>,
    _m: PhantomData<T>,
}

impl<T> WeakHandle<T> {
    fn data(&self) -> &RefCount {
        unsafe { self.references.as_ref() }
    }

    pub fn upgrade(&self) -> Option<Handle<T>> {
        let mut n = self.data().data_references.load(Ordering::Relaxed);
        loop {
            if n == 0 {
                return None;
            }
            if let Err(e) = self.data().data_references.compare_exchange_weak(
                n,
                n + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                n = e;
                continue;
            }
            return Some(Handle {
                id: self.id,
                weak: self.clone(),
            });
        }
    }
}

unsafe impl<T> Send for Handle<T> {}
unsafe impl<T> Sync for Handle<T> {}
unsafe impl<T> Send for WeakHandle<T> {}
unsafe impl<T> Sync for WeakHandle<T> {}

impl<T> Drop for Handle<T> {
    fn drop(&mut self) {
        self.weak
            .data()
            .data_references
            .fetch_sub(1, Ordering::Release);
    }
}

impl<T> Drop for WeakHandle<T> {
    fn drop(&mut self) {
        if self.data().weak_references.fetch_sub(1, Ordering::Release) == 1 {
            // last handle
            // ensure that all other drops have finished
            std::sync::atomic::fence(Ordering::Acquire);
            unsafe {
                drop(Box::from_raw(self.references.as_ptr()));
            }
        }
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        self.weak
            .data()
            .data_references
            .fetch_add(1, Ordering::Relaxed);
        Self {
            id: self.id,
            weak: self.weak.clone(),
        }
    }
}

impl<T> Clone for WeakHandle<T> {
    fn clone(&self) -> Self {
        self.data().weak_references.fetch_add(1, Ordering::Relaxed);
        Self {
            id: self.id,
            references: self.references,
            _m: PhantomData,
        }
    }
}

impl<T> Handle<T> {
    fn new(id: AssetId) -> Self {
        Self {
            id,
            weak: unsafe {
                WeakHandle {
                    id,
                    references: NonNull::new_unchecked(Box::leak(Box::new(RefCount {
                        data_references: AtomicUsize::new(1),
                        weak_references: AtomicUsize::new(1),
                    }))),
                    _m: PhantomData,
                }
            },
        }
    }

    pub fn id(&self) -> AssetId {
        self.id
    }

    pub fn downgrade(&self) -> WeakHandle<T> {
        let weak = self.weak.clone();
        weak
    }
}

pub struct Assets<T> {
    assets: HashMap<AssetId, AssetEntry<T>>,
    next_id: AssetId,
}

impl<T> Default for Assets<T> {
    fn default() -> Self {
        Self {
            assets: Default::default(),
            next_id: 0,
        }
    }
}

impl<T> Assets<T> {
    pub fn insert(&mut self, val: T) -> Handle<T> {
        let id = self.next_id;
        self.next_id += 1;
        let handle = Handle::new(id);
        let _old = self.assets.insert(
            id,
            AssetEntry {
                val,
                handle: handle.downgrade(),
            },
        );
        debug_assert!(_old.is_none());
        debug!(
            id = tracing::field::debug(handle.id()),
            ty = std::any::type_name::<T>(),
            "Inserted new asset"
        );
        handle
    }

    pub fn iter(&self) -> impl Iterator<Item = (AssetId, &T)> {
        self.assets.iter().map(|(id, entry)| (*id, &entry.val))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (AssetId, &mut T)> {
        self.assets
            .iter_mut()
            .map(|(id, entry)| (*id, &mut entry.val))
    }

    pub fn contains(&self, id: AssetId) -> bool {
        self.assets.contains_key(&id)
    }

    pub fn get_by_id(&self, id: AssetId) -> Option<&T> {
        self.assets.get(&id).map(|val| &val.val)
    }

    pub fn get_by_id_mut(&mut self, id: AssetId) -> Option<&mut T> {
        self.assets.get_mut(&id).map(|val| &mut val.val)
    }

    pub fn get(&self, handle: &Handle<T>) -> &T {
        self.assets
            .get(&handle.id)
            .map(|val| &val.val)
            .expect("Handle was invalid")
    }

    pub fn get_mut(&mut self, handle: &Handle<T>) -> &mut T {
        self.assets
            .get_mut(&handle.id)
            .map(|val| &mut val.val)
            .expect("Handle was invalid")
    }
}

struct AssetEntry<T> {
    val: T,
    handle: WeakHandle<T>,
}

fn gc_assets<T: 'static>(mut assets: ResMut<Assets<T>>) {
    assets
        .assets
        .retain(|_id, val| val.handle.data().data_references.load(Ordering::Relaxed) > 0);
}

pub struct AssetsPlugin<T> {
    _m: PhantomData<T>,
}

impl<T> Default for AssetsPlugin<T> {
    fn default() -> Self {
        Self { _m: PhantomData }
    }
}

impl<T: Component> Plugin for AssetsPlugin<T> {
    fn build(self, app: &mut crate::App) {
        app.insert_resource(Assets::<T>::default());
        app.stage(crate::Stage::Update).add_system(gc_assets::<T>);
    }
}
