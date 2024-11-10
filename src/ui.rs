use crate::{
    assets::{self, AssetsPlugin, Handle},
    MouseInputs, Plugin,
};

pub mod color_rect_pipeline;
pub mod rect;
pub mod text;
pub mod text_rect_pipeline;

use std::{collections::HashMap, ptr::NonNull};

use cecs::{prelude::*, query};
use text_rect_pipeline::{DrawTextRect, TextRectRequests};
use winit::event::MouseButton;

use {
    color_rect_pipeline::{DrawColorRect, RectRequests},
    rect::UiRect,
    text::{OwnedTypeFace, TextDrawResponse},
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(color_rect_pipeline::UiColorRectPlugin);
        app.add_plugin(text_rect_pipeline::UiTextRectPlugin);
        let font = text::parse_font(
            include_bytes!("./ui/Roboto-Regular.ttf")
                .to_vec()
                .into_boxed_slice(),
            0,
        )
        .unwrap();
        app.insert_resource(UiState::new(font));
        app.insert_resource(TextTextureCache::default());
        app.insert_resource(Theme {
            primary_color: 0xcdd6f4ff,
            secondary_color: 0x313244ff,
            button_hovered: 0x45475aff,
            button_pressed: 0x585b70ff,
        });
        app.add_startup_system(setup);
        app.add_plugin(AssetsPlugin::<ShapingResult>::default());
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

#[derive(Default)]
pub struct TextTextureCache(pub HashMap<ShapeKey, assets::Handle<ShapingResult>>);

#[derive(Debug)]
pub struct ShapingResult {
    pub unicodebuffer: rustybuzz::UnicodeBuffer,
    pub glyphs: rustybuzz::GlyphBuffer,
    pub texture: TextDrawResponse,
}

/// UI context object. Use this to builder your user interface
pub struct UiState {
    hovered: UiId,
    active: UiId,
    /// stack of parents in the ui tree
    id_stack: Vec<IdType>,

    colored_rects: Vec<DrawColorRect>,
    text_rects: Vec<DrawTextRect>,
    bounds: UiRect,

    font: OwnedTypeFace,

    /// layers go from back to front
    layer: u16,

    bounding_boxes: HashMap<UiId, UiRect>,
}

#[derive(Debug)]
struct Theme {
    pub primary_color: u32,
    pub secondary_color: u32,
    pub button_hovered: u32,
    pub button_pressed: u32,
}

const FONT_SIZE: u32 = 12;
const PADDING: u32 = 5;

impl UiState {
    pub fn new(font: OwnedTypeFace) -> Self {
        Self {
            hovered: Default::default(),
            active: Default::default(),
            id_stack: Default::default(),
            colored_rects: Default::default(),
            text_rects: Default::default(),
            bounds: Default::default(),
            layer: 0,
            font,
            bounding_boxes: Default::default(),
        }
    }
}

impl<'a> Ui<'a> {
    #[inline]
    fn set_hovered(&mut self, id: UiId) {
        self.ui.hovered = id;
    }

    #[inline]
    fn set_active(&mut self, id: UiId) {
        self.ui.active = id;
    }

    #[inline]
    fn parent(&self) -> IdType {
        if self.ui.id_stack.len() >= 2 {
            self.ui.id_stack[self.ui.id_stack.len() - 2]
        } else {
            SENTINEL
        }
    }

    #[inline]
    fn current_idx(&self) -> IdType {
        assert!(!self.ui.id_stack.is_empty());
        unsafe { *self.ui.id_stack.last().unwrap_unchecked() }
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
        self.ui.active == id
    }

    #[inline]
    fn is_hovered(&self, id: UiId) -> bool {
        self.ui.hovered == id
    }

    #[inline]
    fn mouse_up(&self) -> bool {
        self.mouse.just_released.contains(&MouseButton::Left)
    }

    #[inline]
    fn mouse_down(&self) -> bool {
        self.mouse.pressed.contains(&MouseButton::Left)
    }

    #[inline]
    fn contains_mouse(&self, id: UiId) -> bool {
        let Some(bbox) = self.ui.bounding_boxes.get(&id) else {
            return false;
        };

        let mouse = self.mouse.cursor_position;
        let dx = mouse.x - bbox.x as f64;
        let dy = mouse.y - bbox.y as f64;

        0.0 <= dx && dx < bbox.w as f64 && 0.0 <= dy && dy < bbox.h as f64
    }

    #[inline]
    fn set_not_active(&mut self, id: UiId) {
        if self.ui.active == id {
            self.ui.active = UiId::SENTINEL;
        }
    }

    #[inline]
    fn set_not_hovered(&mut self, id: UiId) {
        if self.ui.hovered == id {
            self.ui.hovered = UiId::SENTINEL;
        }
    }

    // TODO: alignment or position
    pub fn panel(&mut self, width: u32, height: u32, mut contents: impl FnMut(&mut Self)) {
        self.ui.id_stack.push(0);
        self.ui.bounds = UiRect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        };
        let layer = self.ui.layer;
        self.ui.layer += 1;
        self.color_rect(0, 0, width, height, 0x04a5e5ff, self.ui.layer);
        contents(self);
        self.ui.layer = layer;
        self.ui.id_stack.pop();
    }

    pub fn grid<'b>(&mut self, columns: u32, mut contents: impl FnMut(Columns) + 'b)
    where
        'a: 'b,
    {
        self.ui.id_stack.push(0);
        let bounds = self.ui.bounds;
        let width = bounds.w / columns + 1;

        let dims = (0..columns)
            .map(|i| [bounds.x + i * width, bounds.x + (i + 1) * width])
            .collect();

        contents(Columns {
            ctx: self.into(),
            cols: columns,
            dims,
        });
        self.ui.id_stack.pop();
    }

    pub fn color_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: u32, layer: u16) {
        self.ui.colored_rects.push(DrawColorRect {
            x,
            y,
            w: width,
            h: height,
            color,
            layer,
        })
    }

    pub fn text_rect(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: u32,
        layer: u16,
        shaping: Handle<ShapingResult>,
    ) {
        self.ui.text_rects.push(DrawTextRect {
            x,
            y,
            w: width,
            h: height,
            color,
            layer,
            shaping,
        })
    }

    fn shape_and_draw_line(
        &mut self,
        line: String,
        size: u32,
    ) -> (Handle<ShapingResult>, &mut ShapingResult) {
        let handle = self
            .texture_cache
            .0
            .entry(ShapeKey {
                text: line.clone(),
                size,
            })
            .or_insert_with(|| {
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line);
                let glyphs = rustybuzz::shape(self.ui.font.face(), &[], buffer);

                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line);
                let pic = text::draw_glyph_buffer(self.ui.font.face(), &glyphs, size).unwrap();

                let shaping = ShapingResult {
                    unicodebuffer: buffer,
                    glyphs,
                    texture: pic,
                };

                self.shaping_results.insert(shaping)
            });

        let shape = self.shaping_results.get_mut(handle);
        (handle.clone(), shape)
    }

    pub fn label(&mut self, label: impl Into<String>) -> Response<()> {
        self.begin_widget();
        let id = self.current_id();
        let layer = self.ui.layer;
        let label = label.into();

        // shape the text
        let mut w = 0;
        let mut h = 0;
        let x = self.ui.bounds.x;
        let y = self.ui.bounds.y;
        let [x, y] = [x + PADDING, y + PADDING];
        let mut text_y = y;
        for line in label.split('\n').filter(|l| !l.is_empty()) {
            let (handle, e) = self.shape_and_draw_line(line.to_owned(), FONT_SIZE);
            let pic = &e.texture;
            w = w.max(pic.width());
            h += pic.height();
            let ph = pic.height();

            self.text_rect(
                x,
                text_y,
                w,
                h,
                self.theme.secondary_color,
                layer + 1,
                handle,
            );
            text_y += ph;
        }
        self.ui.bounds.y += h + 2 * PADDING;
        let rect = UiRect { x, y, w, h };
        self.update_rect(rect);
        Response {
            hovered: self.ui.hovered == id,
            active: self.ui.active == id,
            inner: (),
            rect,
        }
    }

    fn update_rect(&mut self, rect: UiRect) {
        let id = self.current_id();
        self.ui.bounding_boxes.insert(id, rect);
    }

    fn begin_widget(&mut self) {
        *self.ui.id_stack.last_mut().unwrap() += 1;
    }

    pub fn button(&mut self, label: impl Into<String>) -> ButtonResponse {
        self.begin_widget();
        let layer = self.ui.layer;
        let label = label.into();

        let id = self.current_id();
        let mut pressed = false;
        let contains_mouse = self.contains_mouse(id);
        let mut color = self.theme.secondary_color;
        let active = self.is_active(id);
        if active {
            color = self.theme.button_pressed;
            if self.mouse_up() {
                if self.is_hovered(id) {
                    pressed = true;
                }
                self.set_not_active(id);
            }
        } else if self.is_hovered(id) {
            color = self.theme.button_hovered;
            if !contains_mouse {
                self.set_not_hovered(id);
            } else if self.mouse_down() {
                self.set_active(id);
            }
        }
        if contains_mouse {
            self.set_hovered(id);
        }

        const TEXT_PADDING: u32 = 5;
        // shape the text
        let mut w = 0;
        let mut h = 0;
        let x = self.ui.bounds.x;
        let y = self.ui.bounds.y;
        let [x, y] = [x + PADDING, y + PADDING];
        let mut text_y = y + TEXT_PADDING;
        for line in label.split('\n').filter(|l| !l.is_empty()) {
            let (handle, e) = self.shape_and_draw_line(line.to_owned(), FONT_SIZE);
            let pic = &e.texture;
            w = w.max(pic.width());
            h += pic.height();
            let ph = pic.height();

            if !active {
                // add a shadow
                self.text_rect(
                    x + TEXT_PADDING + 1,
                    text_y + 1,
                    w,
                    h,
                    0x000000FF,
                    layer + 1,
                    handle.clone(),
                );
            }
            self.text_rect(
                x + TEXT_PADDING,
                text_y,
                w,
                h,
                self.theme.primary_color,
                layer + 2,
                handle,
            );
            text_y += ph;
        }
        self.ui.bounds.y += h + 2 * PADDING + 2 * TEXT_PADDING;
        // background
        let w = w + 2 * TEXT_PADDING;
        let h = h + 2 * TEXT_PADDING;
        self.color_rect(x, y, w, h, color, layer);

        let rect = UiRect { x, y, w, h };
        self.update_rect(rect);
        ButtonResponse {
            inner: Response {
                hovered: self.ui.hovered == id,
                active: self.ui.active == id,
                inner: (),
                rect,
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
    ctx: NonNull<Ui<'a>>,
    cols: u32,
    dims: Vec<[u32; 2]>,
}

impl<'a> Columns<'a> {
    pub fn column(&mut self, i: u32, mut contents: impl FnMut(&mut Ui)) {
        assert!(i < self.cols);
        let ctx = unsafe { self.ctx.as_mut() };
        let idx = i as usize;
        let bounds = ctx.ui.bounds;
        ctx.ui.bounds.x = self.dims[idx][0];
        ctx.ui.bounds.w = self.dims[idx][1] - self.dims[idx][0];
        let w = ctx.ui.bounds.w;
        *ctx.ui.id_stack.last_mut().unwrap() = i;
        let layer = ctx.ui.layer;
        ctx.ui.layer += 1;
        ctx.ui.id_stack.push(0);
        contents(ctx);
        ctx.ui.id_stack.pop();
        let rect = ctx.ui.bounds;
        ctx.ui.bounds.y = bounds.y;
        ctx.ui.bounds.h = bounds.h;
        if rect.w > w && i + 1 < self.cols {
            let diff = rect.w - w;
            for d in &mut self.dims[idx + 1..] {
                d[0] += diff;
                d[1] += diff;
            }
        }
        ctx.ui.layer = layer;
    }
}

fn begin_frame(mut ui: ResMut<UiState>, size: Res<crate::renderer::WindowSize>) {
    ui.colored_rects.clear();
    ui.text_rects.clear();
    ui.bounds = UiRect {
        x: 0,
        y: 0,
        w: size.width,
        h: size.height,
    };
    ui.layer = 0;
}

fn submit_frame(
    mut ui: ResMut<UiState>,
    mut rects: Query<&mut RectRequests>,
    mut text_rect: Query<&mut TextRectRequests>,
) {
    if let Some(dst) = rects.single_mut() {
        std::mem::swap(&mut ui.colored_rects, &mut dst.0);
    }
    if let Some(dst) = text_rect.single_mut() {
        std::mem::swap(&mut ui.text_rects, &mut dst.0);
    }
}

fn setup(mut cmd: Commands) {
    cmd.spawn().insert(RectRequests::default());
    cmd.spawn().insert(TextRectRequests::default());
}

pub struct Ui<'a> {
    ui: ResMut<'a, UiState>,
    texture_cache: ResMut<'a, TextTextureCache>,
    shaping_results: ResMut<'a, assets::Assets<ShapingResult>>,
    theme: ResMut<'a, Theme>,
    mouse: Res<'a, MouseInputs>,
}

unsafe impl<'a> query::WorldQuery<'a> for Ui<'a> {
    fn new(db: &'a World, _system_idx: usize) -> Self {
        let ui = ResMut::new(db);
        let texture_cache = ResMut::new(db);
        let text_assets = ResMut::new(db);
        let theme = ResMut::new(db);
        let mouse = Res::new(db);
        Self {
            ui,
            texture_cache,
            shaping_results: text_assets,
            theme,
            mouse,
        }
    }

    fn resources_mut(set: &mut std::collections::HashSet<std::any::TypeId>) {
        set.insert(std::any::TypeId::of::<UiState>());
        set.insert(std::any::TypeId::of::<TextTextureCache>());
        set.insert(std::any::TypeId::of::<assets::Assets<ShapingResult>>());
        set.insert(std::any::TypeId::of::<Theme>());
    }

    fn resources_const(set: &mut std::collections::HashSet<std::any::TypeId>) {
        set.insert(std::any::TypeId::of::<MouseInputs>());
    }
}
