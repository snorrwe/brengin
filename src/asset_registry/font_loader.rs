//! Bundled DynamicImage loader

use tracing::error;

use crate::{
    asset_registry::{AssetLoadError, AssetLoader},
    ui::text::OwnedTypeFace,
};

pub struct FontLoader;

impl AssetLoader<OwnedTypeFace> for FontLoader {
    fn load(
        &self,
        path: std::path::PathBuf,
    ) -> impl std::future::Future<Output = Result<OwnedTypeFace, AssetLoadError>> + Send {
        async move {
            crate::ui::text::load_font(path, 0).map_err(|err| {
                error!(?err, "Failed to load font");
                AssetLoadError::LoadFailed(err.to_string())
            })
        }
    }
}
