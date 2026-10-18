use std::marker::PhantomData;

#[repr(transparent)]
pub struct AssetId<T> {
    id: u64,
    _m: PhantomData<T>,
}

impl<T> Default for AssetId<T> {
    fn default() -> Self {
        Self {
            id: ASSET_ID_SENTINEL,
            _m: Default::default(),
        }
    }
}

impl<T> std::fmt::Debug for AssetId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AssetId")
            .field(&std::any::type_name::<T>())
            .field(&self.id)
            .finish()
    }
}

impl<T> Ord for AssetId<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl<T> Eq for AssetId<T> {}

impl<T> PartialOrd for AssetId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl<T> PartialEq for AssetId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> std::hash::Hash for AssetId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Copy for AssetId<T> {}

impl<T> Clone for AssetId<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _m: PhantomData,
        }
    }
}

impl<T> AssetId<T> {
    pub const SENTINEL: Self = Self {
        id: ASSET_ID_SENTINEL,
        _m: PhantomData,
    };

    pub fn new(id: u64) -> Self {
        Self {
            id,
            _m: PhantomData,
        }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }
}

pub const ASSET_ID_SENTINEL: u64 = u64::MAX;
