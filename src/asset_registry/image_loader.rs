//! Bundled DynamicImage loader

use image::DynamicImage;

use crate::asset_registry::{AssetLoadError, AssetLoader};

pub struct DynamicImageLoader;

impl AssetLoader<DynamicImage> for DynamicImageLoader {
    fn load(
        &self,
        path: std::path::PathBuf,
    ) -> impl std::future::Future<Output = Result<DynamicImage, super::AssetLoadError>> + Send {
        async move {
            image::ImageReader::open(&path)
                .map_err(|_| AssetLoadError::FileNotFound(path))?
                .decode()
                .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))
        }
    }
}
