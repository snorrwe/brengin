use std::{collections::HashMap, path::Path, pin::Pin};

use anyhow::Context;
use rustybuzz::GlyphBuffer;

use super::rect::UiRect;

pub struct OwnedTypeFace {
    _data: Pin<Box<[u8]>>,
    face_index: u32,
    face: rustybuzz::Face<'static>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ShapeKey {
    pub text: String,
    pub size: u32,
    // TODO: include font handle, shaping info etc
}

// TODO: GC, assets?
#[derive(Debug, Default)]
pub struct ShapeCache(pub HashMap<ShapeKey, GlyphBuffer>);

#[derive(Debug, Default)]
pub struct TextTextureCache(pub HashMap<ShapeKey, TextDrawResponse>);

impl std::fmt::Debug for OwnedTypeFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("OwnedTypeFace");
        d.finish()
    }
}

impl OwnedTypeFace {
    pub fn face(&self) -> &rustybuzz::Face<'static> {
        &self.face
    }

    pub fn face_mut(&mut self) -> &mut rustybuzz::Face<'static> {
        &mut self.face
    }

    pub fn face_index(&self) -> u32 {
        self.face_index
    }
}

pub fn load_font(path: impl AsRef<Path>, face_index: u32) -> anyhow::Result<OwnedTypeFace> {
    let data = std::fs::read(path.as_ref())
        .with_context(|| format!("Failed to load {:?}", path.as_ref()))?;
    let data = Pin::new(data.into_boxed_slice());
    let face = rustybuzz::Face::from_slice(&data[..], face_index)
        .with_context(|| format!("Failed to parse font {:?}", path.as_ref()))?;

    let face: rustybuzz::Face<'static> = unsafe { std::mem::transmute(face) };

    Ok(OwnedTypeFace {
        _data: data,
        face_index,
        face,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphBufferBounds {
    pub bounds: UiRect,
    pub padding_x: u32,
    pub padding_y: u32,
}

pub fn get_bounds(face: &rustybuzz::Face, glyphs: &GlyphBuffer) -> GlyphBufferBounds {
    let info = glyphs.glyph_infos();
    let pos = glyphs.glyph_positions();

    let mut maxx = 0;
    let mut maxy = 0;
    let mut padding_x = 0;
    let mut padding_y = 0;
    for (pos, info) in pos.into_iter().zip(info.into_iter()) {
        let glyph_id = info.glyph_id;
        let bounds = face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(glyph_id as u16));
        if let Some(bounds) = bounds {
            if bounds.x_min < 0 {
                padding_x = padding_x.max(-bounds.x_min as u32);
            }
            if bounds.y_min < 0 {
                padding_y = padding_y.max(-bounds.y_min as u32);
            }
            if bounds.x_max as i32 > pos.x_advance {
                maxx += bounds.x_max as i32 - pos.x_advance;
            }
            if bounds.y_max as i32 > pos.y_advance {
                maxy = maxy.max(bounds.y_max as i32);
            }
        }
        maxx += pos.x_advance as i32;
        maxy += pos.y_advance as i32;
    }
    let extx = maxx / 2;
    let exty = maxy / 2;

    GlyphBufferBounds {
        bounds: UiRect {
            x: extx as u32,
            y: exty as u32,
            w: maxx as u32,
            h: maxy as u32,
        },
        padding_x,
        padding_y,
    }
}

#[derive(Debug, Clone)]
pub struct TextDrawResponse {
    pub pixmap: tiny_skia::Pixmap,
    pub xoffset: i32,
    pub yoffset: i32,
}

impl TextDrawResponse {
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }
}

pub fn draw_glyph_buffer(
    face: &rustybuzz::Face,
    glyphs: &GlyphBuffer,
    height: u32,
) -> anyhow::Result<TextDrawResponse> {
    let bounds = get_bounds(face, glyphs);

    let scaling_factor = height as f32 / bounds.bounds.h as f32;

    let mut builder = TextOutlineBuilder::new();
    builder.scaling_factor = scaling_factor;

    builder.xoffset = bounds.padding_x as f32 * builder.scaling_factor;
    builder.yoffset = bounds.padding_y as f32 * builder.scaling_factor;

    let mut pixmap = tiny_skia::Pixmap::new(
        ((bounds.bounds.w + bounds.padding_x) as f32 * builder.scaling_factor) as u32,
        ((bounds.bounds.h + bounds.padding_y) as f32 * builder.scaling_factor) as u32,
    )
    .context("Failed to create pixmap")?;

    let info = glyphs.glyph_infos();
    let pos = glyphs.glyph_positions();
    for (pos, info) in pos.into_iter().zip(info.into_iter()) {
        let glyph_id = info.glyph_id;
        face.outline_glyph(
            rustybuzz::ttf_parser::GlyphId(glyph_id as u16),
            &mut builder,
        );
        builder.xoffset += pos.x_advance as f32 * builder.scaling_factor;
        builder.draw(0xFFFFFFFF, &mut pixmap);
    }
    Ok(TextDrawResponse {
        pixmap,
        xoffset: -(bounds.padding_x as i32),
        yoffset: -(bounds.padding_y as i32),
    })
}

pub struct TextOutlineBuilder {
    pub pb: tiny_skia::PathBuilder,
    pub scaling_factor: f32,
    pub xoffset: f32,
    pub yoffset: f32,
}

impl TextOutlineBuilder {
    pub fn new() -> Self {
        let pb = tiny_skia::PathBuilder::new();
        Self {
            pb,
            scaling_factor: 1.0,
            xoffset: 0.0,
            yoffset: 0.0,
        }
    }

    pub fn draw(&mut self, color: u32, pixmap: &mut tiny_skia::Pixmap) {
        let pb = std::mem::replace(&mut self.pb, tiny_skia::PathBuilder::new());
        let Some(path) = pb.finish() else {
            return;
        };

        let r = (color >> 24) & 0xFF;
        let g = (color >> 16) & 0xFF;
        let b = (color >> 8) & 0xFF;
        let a = (color >> 0) & 0xFF;

        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(r as u8, g as u8, b as u8, a as u8);

        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    #[inline]
    fn xpos(&self, x: f32) -> f32 {
        x * self.scaling_factor + self.xoffset
    }

    #[inline]
    fn ypos(&self, y: f32) -> f32 {
        y * self.scaling_factor + self.yoffset
    }
}

impl Default for TextOutlineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl rustybuzz::ttf_parser::OutlineBuilder for TextOutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.xpos(x), self.ypos(y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.xpos(x), self.ypos(y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb
            .quad_to(self.xpos(x1), self.ypos(y1), self.xpos(x), self.ypos(y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.xpos(x1),
            self.ypos(y1),
            self.xpos(x2),
            self.ypos(y2),
            self.xpos(x),
            self.ypos(y),
        );
    }

    fn close(&mut self) {
        self.pb.close();
    }
}
