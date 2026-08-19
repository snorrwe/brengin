// TODO:
// gc freed rects
// use the atlas in renderers

use std::collections::BTreeMap;

use image::DynamicImage;

use super::texture::Texture;
use crate::{prelude::*, renderer::GraphicsState};

pub struct TextureAtlas {
    texture: Texture,
    allocator: guillotiere::AtlasAllocator,
}

impl TextureAtlas {
    pub fn from_texture(texture: Texture) -> Self {
        let allocator = guillotiere::AtlasAllocator::new(guillotiere::euclid::Size2D::new(
            texture.size.0 as i32,
            texture.size.1 as i32,
        ));
        Self { texture, allocator }
    }

    fn allocate(&mut self, width: i32, height: i32) -> Option<guillotiere::Allocation> {
        self.allocator
            .allocate(guillotiere::euclid::Size2D::new(width, height))
    }

    fn deallocate(&mut self, rect: guillotiere::AllocId) {
        self.allocator.deallocate(rect);
    }
}

#[derive(Clone)]
pub struct AtlasRect {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    node_id: guillotiere::AllocId,
    asset_handle: Handle<TextureAtlas>,
}

impl AtlasRect {
    pub fn width(&self) -> i32 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> i32 {
        self.max_y - self.min_y
    }

    pub fn min_x(&self) -> i32 {
        self.min_x
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn max_x(&self) -> i32 {
        self.max_x
    }

    pub fn max_y(&self) -> i32 {
        self.max_y
    }

    pub fn atlas_handle(&self) -> &Handle<TextureAtlas> {
        &self.asset_handle
    }
}

/// Primary way to use texture atlases
pub struct TextureAtlasRegistry<'a> {
    atlases: ResMut<'a, Assets<TextureAtlas>>,
    ids: ResMut<'a, Atlases>,
    renderer: Res<'a, GraphicsState>,
}

#[derive(Default)]
struct Atlases {
    /// id -> Handle mapping
    pub atlas_ids: BTreeMap<u64, WeakHandle<TextureAtlas>>,
}

impl TextureAtlasRegistry<'_> {
    pub fn allocate(&mut self, width: i32, height: i32) -> Option<AtlasRect> {
        for (id, atlas) in self.atlases.iter_mut() {
            if let Some(alloc) = atlas.allocate(width, height)
                && let Some(asset_handle) = self.ids.atlas_ids[&id].upgrade()
            {
                return Some(AtlasRect {
                    min_x: alloc.rectangle.min.x,
                    min_y: alloc.rectangle.min.y,
                    max_x: alloc.rectangle.max.x,
                    max_y: alloc.rectangle.max.y,
                    node_id: alloc.id,
                    asset_handle,
                });
            }
        }

        // no atlas found, attempt to allocate a new
        let mut atlas = {
            let width = width.max(1024);
            let height = height.max(1024);

            let temp = vec![0u8; 4 * width as usize * height as usize];
            let texture = Texture::from_rgba8(
                self.renderer.device(),
                self.renderer.queue(),
                &temp,
                (width as u32, height as u32),
                None,
            )
            .inspect_err(|_err| {
                #[cfg(feature = "tracing")]
                tracing::error!(error=?_err, "Failed to allocate atlas texture");
            })
            .ok()?;

            TextureAtlas::from_texture(texture)
        };
        let alloc = atlas.allocate(width, height)?;

        let handle = self.atlases.insert(atlas);
        self.ids.atlas_ids.insert(handle.id(), handle.downgrade());

        Some(AtlasRect {
            min_x: alloc.rectangle.min.x,
            min_y: alloc.rectangle.min.y,
            max_x: alloc.rectangle.max.x,
            max_y: alloc.rectangle.max.y,
            node_id: alloc.id,
            asset_handle: handle,
        })
    }

    pub fn upload(&mut self, rect: AtlasRect, image: &DynamicImage) {
        let texture_atlas = self.atlases.get(&rect.asset_handle);
        let texture = &texture_atlas.texture.texture;

        let rgba = image.to_rgba8();

        let width = image.width().min(rect.width() as u32);
        let height = image.height().min(rect.height() as u32);

        self.renderer.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::All,
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min_x as u32,
                    y: rect.min_y as u32,
                    z: 0,
                },
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width as u32),
                rows_per_image: Some(height as u32),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    // TODO: RAII / GC unused rects
    pub fn deallocate(&mut self, rect: AtlasRect) {
        let atlas = self.atlases.get_mut(&rect.asset_handle);
        atlas.deallocate(rect.node_id);
    }
}

pub struct TextureAtlasPlugin;

impl Plugin for TextureAtlasPlugin {
    fn build(self, app: &mut App) {
        app.add_plugin(AssetsPlugin::<TextureAtlas>::default());
        app.insert_resource(Atlases::default());
    }
}
