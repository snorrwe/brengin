use glam::Vec2;

use crate::{
    asset_registry::{AssetLoadError, AssetLoader},
    prelude::*,
    renderer::sprite_renderer::SpriteSheet,
};
use std::{fs::OpenOptions, io::BufReader, path::PathBuf};

#[derive(Debug, serde_derive::Deserialize)]
pub struct Vec2Disk {
    pub x: f32,
    pub y: f32,
}

/// On-disk format of spritesheet metadata
/// Loader loads json format
#[derive(Debug, serde_derive::Deserialize)]
pub struct SpriteSheetDisk {
    /// Padding applied to the box
    pub padding: Option<Vec2Disk>,
    /// Size of the entire box
    pub box_size: Vec2Disk,
    /// Number of boxes in a row
    pub num_cols: u32,
    /// Relative path from this file to the image
    pub image: PathBuf,
    /// Relative path from this file to a binary image with the same dimensions as image
    pub mask: Option<PathBuf>,
}

pub struct SpriteSheetLoader;
impl AssetLoader<SpriteSheet> for SpriteSheetLoader {
    fn load(
        &self,
        path: PathBuf,
    ) -> impl std::future::Future<Output = Result<SpriteSheet, AssetLoadError>> + Send {
        async move {
            if !std::fs::exists(&path).unwrap_or(false) {
                return Err(AssetLoadError::FileNotFound(path));
            }
            let f = OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(|_| AssetLoadError::FileNotFound(path.clone()))?;
            let r = BufReader::new(f);

            let data: SpriteSheetDisk = serde_json::from_reader(r)
                .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?;

            // if load succeeded we assume that the path has a parent (directory)
            let img_path = path.parent().unwrap().join(&data.image);
            let image = image::ImageReader::open(&img_path)
                .map_err(|_| AssetLoadError::FileNotFound(img_path))?
                .decode()
                .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?;

            let mask_image = if let Some(mask_path) =
                data.mask.as_ref().map(|p| path.parent().unwrap().join(&p))
            {
                let mask = image::ImageReader::open(&mask_path)
                    .map_err(|_| AssetLoadError::FileNotFound(mask_path))?
                    .decode()
                    .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?;
                Some(mask)
            } else {
                None
            };

            Ok(SpriteSheet {
                num_cols: data.num_cols,
                padding: data
                    .padding
                    .map(|p| Vec2::new(p.x, p.y))
                    .unwrap_or_default(),
                box_size: Vec2::new(data.box_size.x, data.box_size.y),
                size: Vec2::new(image.width() as f32, image.height() as f32),
                image,
                mask: mask_image,
            })
        }
    }
}

pub struct SpriteSheetLoaderPlugin;

impl Plugin for SpriteSheetLoaderPlugin {
    fn build(self, app: &mut App) {
        app.add_plugin(
            super::asset_loader::AssetLoaderPlugin::<SpriteSheet, _>::new(SpriteSheetLoader),
        );
    }
}
