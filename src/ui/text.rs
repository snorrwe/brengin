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
    // TODO: include font handle, shaping info etc
}

// TODO: GC, assets?
#[derive(Debug, Default)]
pub struct ShapeCache(pub HashMap<ShapeKey, GlyphBuffer>);

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

pub fn get_bounds(face: &rustybuzz::Face, glyphs: &GlyphBuffer) -> Option<UiRect> {
    let info = glyphs.glyph_infos();
    let pos = glyphs.glyph_positions();

    let mut maxx = 0;
    let mut has = false;
    for (pos, info) in pos.into_iter().zip(info.into_iter()) {
        let glyph_id = info.glyph_id;
        let bounds = face.outline_glyph(
            rustybuzz::ttf_parser::GlyphId(glyph_id as u16),
            &mut NoopBuilder,
        );
        if let Some(_bounds) = bounds {
            maxx += pos.x_advance;
            has = true;
        }
    }
    has.then(|| {
        let extx = maxx / 2;
        let exty = maxx / 2;

        UiRect {
            x: extx as u32,
            y: exty as u32,
            w: maxx as u32,
            h: face.height() as u32,
        }
    })
}

pub struct NoopBuilder;

impl rustybuzz::ttf_parser::OutlineBuilder for NoopBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = y;
        let _ = x;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let _ = y;
        let _ = x;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = y;
        let _ = x;
        let _ = y1;
        let _ = x1;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = x2;
        let _ = y;
        let _ = x;
        let _ = y2;
        let _ = y1;
        let _ = x1;
    }

    fn close(&mut self) {}
}
