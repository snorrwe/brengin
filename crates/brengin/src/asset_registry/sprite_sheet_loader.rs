use glam::Vec2;

use crate::{
    asset_registry::{AssetLoadError, AssetLoader, FileLoader},
    renderer::sprite_renderer::SpriteSheet,
};
use std::{path::PathBuf, sync::Arc};

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
    async fn load(
        &self,
        path: PathBuf,
        loader: Arc<dyn FileLoader>,
    ) -> Result<SpriteSheet, AssetLoadError> {
        let data = loader.load_all(&path).map_err(|err| {
            #[cfg(feature = "tracing")]
            tracing::error!(?err, "Failed to load sprite sheet bytes");
            AssetLoadError::FileNotFound(path.clone())
        })?;

        let data: SpriteSheetDisk = serde_json::from_reader(std::io::Cursor::new(data))
            .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?;

        // if load succeeded we assume that the path has a parent (directory)
        let img_path = path.parent().unwrap().join(&data.image);
        let image_bytes = loader
            .load_all(&img_path)
            .map_err(|_| AssetLoadError::FileNotFound(img_path))?;
        let image = image::load_from_memory(&image_bytes)
            .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?;

        let mask_image =
            if let Some(mask_path) = data.mask.as_ref().map(|p| path.parent().unwrap().join(p)) {
                let mask_bytes = loader
                    .load_all(&mask_path)
                    .map_err(|_| AssetLoadError::FileNotFound(mask_path))?;
                Some(
                    image::load_from_memory(&mask_bytes)
                        .map_err(|err| AssetLoadError::LoadFailed(err.to_string()))?,
                )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_registry::LoadError;
    use image::GenericImageView;
    use std::{borrow::Cow, collections::HashMap, path::Path};

    struct MemoryLoader(HashMap<PathBuf, Vec<u8>>);

    impl FileLoader for MemoryLoader {
        fn load_all<'a>(&'a self, path: &Path) -> Result<Cow<'a, [u8]>, LoadError> {
            self.0
                .get(path)
                .map(|bytes| Cow::Borrowed(bytes.as_slice()))
                .ok_or_else(|| LoadError::NotFound(path.to_path_buf()))
        }
    }

    #[test]
    fn loads_images_from_file_loader() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 3)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();

        let loader = Arc::new(MemoryLoader(HashMap::from([
            (
                PathBuf::from("sheets/sheet.json"),
                br#"{"box_size":{"x":2,"y":3},"num_cols":1,"image":"sheet.png","mask":"mask.png"}"#
                    .to_vec(),
            ),
            (PathBuf::from("sheets/sheet.png"), png.get_ref().clone()),
            (PathBuf::from("sheets/mask.png"), png.into_inner()),
        ])));

        let sheet =
            pollster::block_on(SpriteSheetLoader.load("sheets/sheet.json".into(), loader)).unwrap();
        assert_eq!(sheet.image.dimensions(), (2, 3));
        assert_eq!(sheet.mask.unwrap().dimensions(), (2, 3));
    }
}
