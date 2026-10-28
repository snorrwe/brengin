//! Bundled DynamicImage loader

use std::sync::Arc;

use image::DynamicImage;

use crate::asset_registry::{AssetLoadError, AssetLoader, FileLoader};

pub struct DynamicImageLoader;

impl AssetLoader<DynamicImage> for DynamicImageLoader {
    async fn load(
        &self,
        path: std::path::PathBuf,
        loader: Arc<dyn FileLoader>,
    ) -> Result<DynamicImage, super::AssetLoadError> {
        let data = loader.load_all(&path).map_err(|err| {
            #[cfg(feature = "tracing")]
            tracing::error!(?err, "Failed to load image bytes");
            AssetLoadError::FileNotFound(path.clone())
        })?;
        let reader = std::io::Cursor::new(&data[..]);
        image::ImageReader::new(reader)
            .with_guessed_format()
            .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?
            .decode()
            .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))
    }
}
