use std::{path::Path, pin::Pin};

use anyhow::Context;

pub struct OwnedTypeFace {
    _data: Pin<Box<[u8]>>,
    face_index: u32,
    face: rustybuzz::Face<'static>,
}

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
