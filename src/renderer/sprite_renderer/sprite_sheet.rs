use cecs::bundle::Bundle;
use glam::Vec2;
use image::DynamicImage;

use crate::{
    assets::Handle,
    color::Color,
    renderer::sprite_renderer::{SpriteInstanceRaw, Visible},
};

pub fn sprite_sheet_bundle(
    handle: Handle<SpriteSheet>,
    instance: impl Into<Option<SpriteInstance>>,
) -> impl Bundle {
    (
        instance.into().unwrap_or(SpriteInstance {
            index: 0,
            flip: false,
            color: Color::TRANSPARENT_BLACK,
        }),
        Visible,
        handle,
        SpriteInstanceRaw::default(),
    )
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteSheetGpu {
    pub padding: [f32; 2],
    pub box_size: [f32; 2],
    pub size: [f32; 2],
    pub num_cols: u32,
    /// rgb color to mask by instances
    pub mask_color: u32,
}

pub struct SpriteSheet {
    /// Padding applied to the box
    pub padding: Vec2,
    /// Size of the entire box
    pub box_size: Vec2,
    /// Number of boxes in a row
    pub num_cols: u32,
    pub image: DynamicImage,
    /// Size of the entire sheet
    pub size: Vec2,
    /// Color to mask by each instance's color.
    /// Transparent black is not supported
    pub mask_color: Option<Color>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (1.0 - t) * a + t * b
}

impl SpriteSheet {
    #[deprecated = "Use from_grid instead"]
    pub fn from_image(padding: Vec2, box_size: Vec2, num_cols: u32, image: DynamicImage) -> Self {
        Self::from_grid(padding, box_size, num_cols, image)
    }

    /// Construct a spritesheet from a uniform grid of sprites
    pub fn from_grid(padding: Vec2, box_size: Vec2, num_cols: u32, image: DynamicImage) -> Self {
        Self {
            padding,
            box_size,
            num_cols,
            size: Vec2::new(image.width() as f32, image.height() as f32),
            image,
            mask_color: None,
        }
    }

    pub fn extract(&self) -> SpriteSheetGpu {
        SpriteSheetGpu {
            padding: self.padding.to_array(),
            box_size: self.box_size.to_array(),
            num_cols: self.num_cols,
            size: self.size.to_array(),
            mask_color: self.mask_color.map(|c| c.0 >> 8).unwrap_or(0),
        }
    }

    /// return the min-max bounding box in image pixel coordinates
    pub fn get_instance_box(&self, instance: SpriteInstance) -> [Vec2; 2] {
        let row: u32 = instance.index / self.num_cols;
        let col: u32 = instance.index - self.num_cols * row;

        let offset = self.box_size * Vec2::new(col as f32, row as f32);
        let mut min = Vec2::ZERO;
        let mut max = Vec2::ONE;

        if instance.flip {
            min.x = 1.0 - min.x;
            max.x = 1.0 - max.x;
        }

        min.x = lerp(self.padding.x, self.box_size.x - self.padding.x, min.x) + offset.x;
        min.y = lerp(self.padding.y, self.box_size.y - self.padding.y, min.y) + offset.y;
        max.x = lerp(self.padding.x, self.box_size.x - self.padding.x, max.x) + offset.x;
        max.y = lerp(self.padding.y, self.box_size.y - self.padding.y, max.y) + offset.y;

        [min, max]
    }

    /// return the min-max bounding box in UV coordinates
    pub fn get_instance_uv(&self, instance: SpriteInstance) -> [Vec2; 2] {
        let [min, max] = self.get_instance_box(instance);
        let div = Vec2::new(self.image.width() as f32, self.image.height() as f32);
        [min / div, max / div]
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct SpriteInstance {
    pub index: u32,
    pub flip: bool,
    /// RGB Color to use when masking the sprite
    /// If sprite masking is disabled on the spritesheet, then
    pub color: Color,
}
