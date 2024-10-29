use crate::Plugin;

pub mod core;
pub mod rect;
pub mod text;

use std::collections::HashMap;

use cecs::prelude::*;

use {
    core::{DrawRect, RectRequests},
    rect::UiRect,
    text::{OwnedTypeFace, TextDrawResponse},
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(core::UiCorePlugin);
        let font = text::load_font("/nix/store/a7xny2d815wb4x4rqrq3fl5dhxrqlxrn-X11-fonts/share/X11/fonts/DejaVuSans-Bold.ttf", 0).unwrap();
        app.insert_resource(Ui::new(font));
        app.add_startup_system(setup);
        app.with_stage(crate::Stage::PreUpdate, |s| {
            s.add_system(begin_frame);
        });
        app.with_stage(crate::Stage::PostUpdate, |s| {
            s.add_system(submit_frame);
        });
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ShapeKey {
    pub text: String,
    pub size: u32,
    // TODO: include font handle, shaping info etc
}

// TODO: GC, assets?
#[derive(Debug, Default)]
pub struct TextTextureCache(pub HashMap<ShapeKey, ShapingResult>);

#[derive(Debug)]
pub struct ShapingResult {
    pub unicodebuffer: rustybuzz::UnicodeBuffer,
    pub glyphs: rustybuzz::GlyphBuffer,
    pub texture: TextDrawResponse,
}

/// UI context object. Use this to builder your user interface
#[derive(Debug)]
pub struct Ui {
    hovered: UiId,
    active: UiId,
    /// stack of parents in the ui tree
    id_stack: Vec<IdType>,

    rects: Vec<DrawRect>,
    bounds: UiRect,

    font: OwnedTypeFace,
    texture_cache: TextTextureCache,

    /// layers go from back to front
    layer: u16,
}

const FONT_SIZE: u32 = 12;
const PADDING: u32 = 5;

impl Ui {
    pub fn new(font: OwnedTypeFace) -> Self {
        Self {
            hovered: Default::default(),
            active: Default::default(),
            id_stack: Default::default(),
            rects: Default::default(),
            bounds: Default::default(),
            texture_cache: Default::default(),
            layer: 0,
            font,
        }
    }

    #[inline]
    fn set_hovered(&mut self, id: UiId) {
        self.hovered = id;
    }

    #[inline]
    fn set_active(&mut self, id: UiId) {
        self.active = id;
    }

    #[inline]
    fn parent(&self) -> IdType {
        if self.id_stack.len() >= 2 {
            self.id_stack[self.id_stack.len() - 2]
        } else {
            SENTINEL
        }
    }

    #[inline]
    fn current_idx(&self) -> IdType {
        assert!(!self.id_stack.is_empty());
        unsafe { *self.id_stack.last().unwrap_unchecked() }
    }

    #[inline]
    fn current_id(&self) -> UiId {
        UiId {
            parent: self.parent(),
            index: self.current_idx(),
        }
    }

    #[inline]
    fn is_active(&self, id: UiId) -> bool {
        self.active == id
    }

    #[inline]
    fn is_hovered(&self, id: UiId) -> bool {
        self.hovered == id
    }

    #[inline]
    fn mouse_up(&self) -> bool {
        // TODO:
        false
    }

    #[inline]
    fn mouse_down(&self) -> bool {
        // TODO:
        false
    }

    #[inline]
    fn contains_mouse(&self, id: UiId) -> bool {
        // TODO:
        false
    }

    #[inline]
    fn set_not_active(&mut self, id: UiId) {
        if self.active == id {
            self.active = UiId::SENTINEL;
        }
    }

    pub fn grid(&mut self, columns: u32, mut contents: impl FnMut(Columns)) {
        self.id_stack.push(0);
        let bounds = self.bounds;
        let width = bounds.w / columns + 1;

        let dims = (0..columns)
            .map(|i| [bounds.x + i * width, bounds.x + (i + 1) * width])
            .collect();

        contents(Columns {
            ctx: self,
            cols: columns,
            dims,
        });
        self.id_stack.pop();
    }

    pub fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: u32, layer: u16) {
        self.rects.push(DrawRect {
            x,
            y,
            w: width,
            h: height,
            color,
            layer,
        })
    }

    fn shape_and_draw_line(&mut self, line: String, size: u32) -> &mut ShapingResult {
        self.texture_cache
            .0
            .entry(ShapeKey {
                text: line.clone(),
                size,
            })
            .or_insert_with(|| {
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line);
                let glyphs = rustybuzz::shape(self.font.face(), &[], buffer);

                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line);
                let pic = text::draw_glyph_buffer(self.font.face(), &glyphs, size).unwrap();

                ShapingResult {
                    unicodebuffer: buffer,
                    glyphs,
                    texture: pic,
                }
            })
    }

    pub fn button(&mut self, label: impl Into<String>) -> ButtonResponse {
        let layer = self.layer;
        let label = label.into();

        let id = self.current_id();
        let mut pressed = false;
        if self.is_active(id) {
            if self.mouse_up() {
                if self.is_hovered(id) {
                    pressed = true;
                }
                self.set_not_active(id);
            }
        } else if self.is_hovered(id) && self.mouse_down() {
            self.set_active(id);
        }
        if self.contains_mouse(id) {
            self.set_hovered(id);
        }

        // test color
        let color = {
            let mut hash = 0x81aaaaaau32;
            for byte in label.as_bytes() {
                hash ^= *byte as u32;
                hash = hash.wrapping_mul(0x1000193);
            }
            hash |= 0xFF;
            hash
        };

        const TEXT_PADDING: u32 = 5;
        // shape the text
        let mut w = 0;
        let mut h = 0;
        for line in label.split('\n').filter(|l| !l.is_empty()) {
            let e = self.shape_and_draw_line(line.to_owned(), FONT_SIZE);
            let pic = &e.texture;
            w = w.max(pic.width());
            h += pic.height();
        }
        let x = self.bounds.x;
        let y = self.bounds.y;
        let [x, y] = [x + PADDING, y + PADDING];
        self.bounds.y += h + 2 * PADDING + 2 * TEXT_PADDING;
        // text
        // TODO: use the textures
        self.rect(
            x + TEXT_PADDING,
            y + TEXT_PADDING,
            w,
            h,
            0x000F0FFF,
            layer + 1,
        );
        // background
        self.rect(
            x,
            y,
            w + 2 * TEXT_PADDING,
            h + 2 * TEXT_PADDING,
            color,
            layer,
        );

        ButtonResponse {
            inner: Response {
                hovered: self.hovered == id,
                active: self.active == id,
                inner: (),
                rect: UiRect { x, y, w, h },
            },
            pressed,
        }
    }
}

type IdType = u32;
const SENTINEL: IdType = !0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UiId {
    parent: IdType,
    index: IdType,
}

impl UiId {
    pub const SENTINEL: UiId = Self {
        parent: SENTINEL,
        index: SENTINEL,
    };
}

impl Default for UiId {
    fn default() -> Self {
        Self::SENTINEL
    }
}

pub struct Response<T> {
    pub hovered: bool,
    pub active: bool,
    pub rect: UiRect,
    pub inner: T,
}

pub struct ButtonResponse {
    pub inner: Response<()>,
    pub pressed: bool,
}

impl ButtonResponse {
    pub fn pressed(&self) -> bool {
        self.pressed
    }
}

pub struct Columns<'a> {
    ctx: &'a mut Ui,
    cols: u32,
    dims: Vec<[u32; 2]>,
}

impl<'a> Columns<'a> {
    pub fn column(&mut self, i: u32, mut contents: impl FnMut(&mut Ui)) {
        assert!(i < self.cols);
        let idx = i as usize;
        let bounds = self.ctx.bounds;
        self.ctx.bounds.x = self.dims[idx][0];
        self.ctx.bounds.w = self.dims[idx][1] - self.dims[idx][0];
        let w = self.ctx.bounds.w;
        *self.ctx.id_stack.last_mut().unwrap() = i;
        let layer = self.ctx.layer;
        self.ctx.layer += 1;
        contents(self.ctx);
        let rect = self.ctx.bounds;
        self.ctx.bounds.y = bounds.y;
        self.ctx.bounds.h = bounds.h;
        if rect.w > w && i + 1 < self.cols {
            let diff = rect.w - w;
            for d in &mut self.dims[idx + 1..] {
                d[0] += diff;
                d[1] += diff;
            }
        }
        self.ctx.layer = layer;
    }
}

fn begin_frame(mut ui: ResMut<Ui>, size: Res<crate::renderer::WindowSize>) {
    ui.rects.clear();
    ui.bounds = UiRect {
        x: 0,
        y: 0,
        w: size.width,
        h: size.height,
    };
    ui.layer = 0;

    // TODO: remove
    std::fs::create_dir_all("target/out").unwrap();
    for (k, v) in ui.texture_cache.0.iter() {
        let pic = &v.texture.pixmap;
        pic.save_png(format!("target/out/{}.png", k.text)).unwrap();
    }
}

fn submit_frame(mut ui: ResMut<Ui>, mut rects: Query<&mut RectRequests>) {
    if let Some(dst) = rects.single_mut() {
        std::mem::swap(&mut ui.rects, &mut dst.0);
    }
}

fn setup(mut cmd: Commands) {
    cmd.spawn().insert(RectRequests::default());
}
