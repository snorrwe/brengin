//! Bundled DynamicImage loader

use std::sync::Arc;

use crate::{
    asset_registry::{AssetLoadError, AssetLoader, FileLoader},
    ui::text::OwnedTypeFace,
};

pub struct FontLoader;

impl AssetLoader<OwnedTypeFace> for FontLoader {
    async fn load(
        &self,
        path: std::path::PathBuf,
        loader: Arc<dyn FileLoader>,
    ) -> Result<OwnedTypeFace, AssetLoadError> {
        let data = loader.load_all(&path).map_err(|err| {
            #[cfg(feature = "tracing")]
            tracing::error!(?err, "Failed to load font");
            AssetLoadError::FileNotFound(path)
        })?;
        crate::ui::text::parse_font(data.to_vec(), 0).map_err(|err| {
            #[cfg(feature = "tracing")]
            tracing::error!(?err, "Failed to load font");
            AssetLoadError::LoadFailed(err.to_string())
        })
    }
}
