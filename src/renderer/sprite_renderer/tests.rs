use glam::Vec2;

use crate::renderer::sprite_renderer::sprite_sheet::{SpriteInstance, SpriteSheet};

#[test]
fn test_sprite_sheet_extract() {
    let img = image::RgbImage::new(64, 64);
    let sheet = SpriteSheet::from_grid(Vec2::ZERO, Vec2::splat(32.0), 2, img.into());

    let uvbox = sheet.get_instance_uv(SpriteInstance {
        index: 0,
        flip: false,
        ..Default::default()
    });

    assert_eq!(uvbox[0], Vec2::new(0.0, 0.0));
    assert_eq!(uvbox[1], Vec2::new(0.5, 0.5));

    let uvbox = sheet.get_instance_uv(SpriteInstance {
        index: 3,
        flip: true,
        ..Default::default()
    });

    assert_eq!(uvbox[0], Vec2::new(1.0, 0.5));
    assert_eq!(uvbox[1], Vec2::new(0.5, 1.0));
}
