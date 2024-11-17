use crate::{
    assets::{self, AssetsPlugin, Handle},
    MouseInputs, Plugin,
};

pub mod color_rect_pipeline;
pub mod rect;
pub mod text;
pub mod text_rect_pipeline;

use std::{any::TypeId, collections::HashMap, ptr::NonNull};

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
        app.insert_resource(UiMemory::default());
        if app.get_resource::<Theme>().is_none() {
            app.insert_resource(Theme::default());
        }
        app.add_plugin(AssetsPlugin::<ShapingResult>::default());
        app.with_stage(crate::Stage::PreUpdate, |s| {
            s.add_system(begin_frame);
        });
        app.with_stage(crate::Stage::PostUpdate, |s| {
            s.add_system(submit_frame_color_rects)
                .add_system(submit_text_rects);
        });
    }
}

fn fnv_1a(value: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for byte in value {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x1000193);
    }
    hash
}

fn fnv_1a_u32(value: u32) -> u32 {
    fnv_1a(bytemuck::cast_slice(&[value]))
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
    pub glyphs: rustybuzz::GlyphBuffer,
    pub texture: TextDrawResponse,
}

/// UI context object. Use this to builder your user interface
pub struct UiState {
    hovered: UiId,
    active: UiId,
    /// stack of parents in the ui tree
    id_stack: Vec<IdxType>,

    color_rects: Vec<DrawColorRect>,
    text_rects: Vec<DrawTextRect>,
    scissors: Vec<UiRect>,
    scissor_idx: u32,
    bounds: UiRect,

    font: OwnedTypeFace,

    /// layers go from back to front
    layer: u16,

    bounding_boxes: HashMap<UiId, UiRect>,

    rect_history: Vec<UiRect>,

    /// hash of the current tree root
    root_hash: u32,

    layout_dir: LayoutDirection,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub primary_color: u32,
    pub secondary_color: u32,
    pub button_hovered: u32,
    pub button_pressed: u32,
    pub text_padding: u32,
    pub font_size: u32,
    pub padding: u32,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            primary_color: 0xcdd6f4ff,
            secondary_color: 0x313244ff,
            button_hovered: 0x45475aff,
            button_pressed: 0x585b70ff,
            text_padding: 5,
            font_size: 12,
            padding: 5,
        }
    }
}

impl UiState {
    pub fn new(font: OwnedTypeFace) -> Self {
        Self {
            hovered: Default::default(),
            active: Default::default(),
            id_stack: Default::default(),
            color_rects: Default::default(),
            text_rects: Default::default(),
            scissors: Default::default(),
            scissor_idx: 0,
            bounds: Default::default(),
            layer: 0,
            font,
            bounding_boxes: Default::default(),
            rect_history: Default::default(),
            root_hash: 0,
            layout_dir: LayoutDirection::TopDown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutDirection {
    TopDown,
    LeftRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCoordinate {
    Absolute(i32),
    Percent(i8),
}

impl Default for UiCoordinate {
    fn default() -> Self {
        Self::Absolute(0)
    }
}

impl From<i32> for UiCoordinate {
    fn from(value: i32) -> Self {
        Self::Absolute(value)
    }
}

impl UiCoordinate {
    pub fn as_abolute(self, max: i32) -> i32 {
        match self {
            UiCoordinate::Absolute(x) => x,
            UiCoordinate::Percent(p) => {
                let p = p as f64 / 100.0;
                let x = max as f64 * p;
                x as i32
            }
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct PanelDescriptor {
    pub width: UiCoordinate,
    pub height: UiCoordinate,
    pub horizonal: HorizontalAlignment,
    pub vertical: VerticalAlignment,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum HorizontalAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum VerticalAlignment {
    #[default]
    Top,
    Center,
    Bottom,
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
    fn parent(&self) -> IdxType {
        if self.ui.id_stack.len() >= 2 {
            self.ui.id_stack[self.ui.id_stack.len() - 2]
        } else {
            SENTINEL
        }
    }

    #[inline]
    fn current_idx(&self) -> IdxType {
        assert!(!self.ui.id_stack.is_empty());
        unsafe { *self.ui.id_stack.last().unwrap_unchecked() }
    }

    #[inline]
    fn current_id(&self) -> UiId {
        let index = self.current_idx();
        let hash = {
            let mut hash = fnv_1a_u32(self.ui.root_hash);
            for i in self.ui.id_stack.iter() {
                hash = fnv_1a(bytemuck::cast_slice(&[hash, *i]));
            }
            hash
        };
        UiId {
            parent: self.parent(),
            index,
            uid: hash,
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

    pub fn horizontal(&mut self, mut contents: impl FnMut(&mut Self)) {
        self.begin_widget();
        let layout = self.ui.layout_dir;
        let history_start = self.ui.rect_history.len();
        let bounds = self.ui.bounds;
        self.ui.layout_dir = LayoutDirection::LeftRight;
        self.ui.id_stack.push(0);
        contents(self);
        self.ui.id_stack.pop();
        self.ui.layout_dir = layout;
        self.ui.bounds = bounds;
        self.submit_rect_group(history_start);
    }

    pub fn vertical(&mut self, mut contents: impl FnMut(&mut Self)) {
        self.begin_widget();
        let layout = self.ui.layout_dir;
        let history_start = self.ui.rect_history.len();
        let bounds = self.ui.bounds;
        self.ui.layout_dir = LayoutDirection::TopDown;
        self.ui.id_stack.push(0);
        contents(self);
        self.ui.id_stack.pop();
        self.ui.layout_dir = layout;
        self.ui.bounds = bounds;
        self.submit_rect_group(history_start);
    }

    fn submit_rect_group(&mut self, history_start: usize) {
        if self.ui.rect_history.len() <= history_start {
            // no rects have been submitted
            return;
        }

        let mut rect = self.ui.rect_history[history_start];
        self.ui.rect_history[history_start + 1..]
            .iter()
            .for_each(|r| rect = rect.grow_over(*r));
        self.submit_rect(self.current_id(), rect);
    }

    pub fn grid<'b>(&mut self, columns: u32, mut contents: impl FnMut(&mut Columns) + 'b)
    where
        'a: 'b,
    {
        self.begin_widget();
        self.ui.id_stack.push(0);
        let cols = columns as i32;
        let history_start = self.ui.rect_history.len();
        let bounds = self.ui.bounds;
        let width = (bounds.w / cols + 1) as i32;

        let dims = (0..cols as i32)
            .map(|i| [bounds.x + i * width, bounds.x + (i + 1) * width])
            .collect();

        let mut cols = Columns {
            ctx: self.into(),
            cols: columns,
            dims,
        };
        contents(&mut cols);

        let mut w = bounds.w as i32;
        for d in cols.dims {
            w = w.max(d[1] - d[0]);
        }
        self.ui.id_stack.pop();
        self.ui.bounds = bounds;
        self.submit_rect_group(history_start);
    }

    pub fn color_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: u32, layer: u16) {
        self.ui.rect_history.push(UiRect {
            x,
            y,
            w: width,
            h: height,
        });
        assert!(!self.ui.scissors.is_empty());
        let scissor = self.ui.scissor_idx;
        self.ui.color_rects.push(DrawColorRect {
            x,
            y,
            w: width,
            h: height,
            color,
            layer,
            scissor,
        })
    }

    pub fn text_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
        layer: u16,
        shaping: Handle<ShapingResult>,
    ) {
        self.ui.rect_history.push(UiRect {
            x,
            y,
            w: width,
            h: height,
        });
        assert!(!self.ui.scissors.is_empty());
        let scissor = self.ui.scissor_idx;
        self.ui.text_rects.push(DrawTextRect {
            x,
            y,
            w: width,
            h: height,
            color,
            layer,
            shaping,
            scissor,
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
                let pic = text::draw_glyph_buffer(self.ui.font.face(), &glyphs, size).unwrap();

                let shaping = ShapingResult {
                    glyphs,
                    texture: pic,
                };

                self.shaping_results.insert(shaping)
            });

        let shape = self.shaping_results.get_mut(handle);
        (handle.clone(), shape)
    }

    pub fn with_theme(&mut self, theme: Theme, mut contents: impl FnMut(&mut Self)) {
        let t = std::mem::replace(&mut *self.theme, theme);

        contents(self);

        *self.theme = t;
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
        let padding = self.theme.padding as i32;
        let [x, y] = [x + padding, y + padding];
        let mut text_y = y;
        for line in label.split('\n').filter(|l| !l.is_empty()) {
            let (handle, e) = self.shape_and_draw_line(line.to_owned(), self.theme.font_size);
            let pic = &e.texture;
            let line_width = pic.width() as i32;
            let line_height = pic.height() as i32;
            w = w.max(line_width);
            h += line_height;
            let ph = pic.height() as i32;

            self.text_rect(
                x,
                text_y,
                line_width,
                line_height,
                self.theme.secondary_color,
                layer + 1,
                handle,
            );
            text_y += ph;
        }
        let rect = UiRect { x, y, w, h };
        self.submit_rect(id, rect);
        Response {
            hovered: self.ui.hovered == id,
            active: self.ui.active == id,
            inner: (),
            rect,
        }
    }

    /// When a widget has been completed, submit its bounding rectangle
    fn submit_rect(&mut self, id: UiId, rect: UiRect) {
        let padding = self.theme.padding as i32;
        match self.ui.layout_dir {
            LayoutDirection::TopDown => {
                let dy = rect.h + 2 * padding;
                self.ui.bounds.y += dy as i32;
                self.ui.bounds.h = self.ui.bounds.h.saturating_sub(dy);
            }
            LayoutDirection::LeftRight => {
                let dx = rect.w + 2 * padding;
                self.ui.bounds.x += dx as i32;
                self.ui.bounds.w = self.ui.bounds.w.saturating_sub(dx);
            }
        }
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

        // shape the text
        let mut w = 0;
        let mut h = 0;
        let x = self.ui.bounds.x as i32;
        let y = self.ui.bounds.y as i32;
        let padding = self.theme.padding as i32;
        let [x, y] = [x + padding, y + padding];
        let text_padding = self.theme.text_padding as i32;
        let mut text_y = y + text_padding;
        for line in label.split('\n').filter(|l| !l.is_empty()) {
            let (handle, e) = self.shape_and_draw_line(line.to_owned(), self.theme.font_size);
            let pic = &e.texture;
            let line_width = pic.width() as i32;
            let line_height = pic.height() as i32;
            w = w.max(line_width);
            h += line_height;

            let mut delta = 0;
            if !active {
                // add a shadow
                self.text_rect(
                    x + text_padding + 1,
                    text_y + 1,
                    line_width,
                    line_height,
                    0x000000FF,
                    layer + 1,
                    handle.clone(),
                );
            } else {
                // if active, then move the text into the shadow's position
                // so it appears to have lowered
                delta = 1
            }
            self.text_rect(
                x + text_padding + delta,
                text_y + delta,
                line_width,
                line_height,
                self.theme.primary_color,
                layer + 2,
                handle,
            );
            text_y += line_height + text_padding;
        }
        // background
        let w = w + 2 * text_padding;
        let h = h + 2 * text_padding;
        self.color_rect(x, y, w, h, color, layer);

        let rect = UiRect { x, y, w, h };
        self.submit_rect(id, rect);
        ButtonResponse {
            hovered: self.ui.hovered == id,
            active: self.ui.active == id,
            inner: ButtonState { pressed },
            rect,
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn theme_mut(&mut self) -> &mut ResMut<'a, Theme> {
        &mut self.theme
    }

    pub fn get_memory_or_default<T: Default + 'static>(&mut self) -> &mut T {
        let key = self.memory_key::<T>();
        let res = self
            .memory
            .0
            .entry(key)
            .or_insert_with(|| ErasedMemoryEntry::new(T::default()));
        unsafe { res.as_inner_mut() }
    }

    pub fn get_memory<T: 'static>(&self) -> Option<&T> {
        let key = self.memory_key::<T>();
        self.memory.0.get(&key).map(|res| unsafe { res.as_inner() })
    }

    pub fn get_memory_mut<T: 'static>(&mut self) -> Option<&mut T> {
        let key = self.memory_key::<T>();
        self.memory
            .0
            .get_mut(&key)
            .map(|res| unsafe { res.as_inner_mut() })
    }

    pub fn insert_memory<T: 'static>(&mut self, item: T) {
        let key = self.memory_key::<T>();
        self.memory.0.insert(key, ErasedMemoryEntry::new(item));
    }

    pub fn remove_memory<T: 'static>(&mut self) -> Option<Box<T>> {
        let key = self.memory_key::<T>();
        self.memory
            .0
            .remove(&key)
            .map(|res| unsafe { res.into_inner() })
    }

    fn memory_key<T: 'static>(&self) -> (UiId, TypeId) {
        (self.current_id(), TypeId::of::<T>())
    }
}

type IdxType = u32;
const SENTINEL: IdxType = !0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UiId {
    parent: IdxType,
    index: IdxType,
    uid: IdxType,
}

impl UiId {
    pub const SENTINEL: UiId = Self {
        parent: SENTINEL,
        index: SENTINEL,
        uid: SENTINEL,
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

pub type ButtonResponse = Response<ButtonState>;

impl ButtonResponse {
    pub fn pressed(&self) -> bool {
        self.inner.pressed
    }
}

pub struct ButtonState {
    pub pressed: bool,
}

pub struct Columns<'a> {
    ctx: NonNull<Ui<'a>>,
    cols: u32,
    /// [x start, x end][cols]
    dims: Vec<[i32; 2]>,
}

impl<'a> Columns<'a> {
    pub fn column(&mut self, i: u32, mut contents: impl FnMut(&mut Ui)) {
        assert!(i < self.cols);
        // setup
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

        // restore state
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
    ui.layout_dir = LayoutDirection::TopDown;
    ui.root_hash = 0;
    ui.rect_history.clear();
    ui.color_rects.clear();
    ui.text_rects.clear();
    ui.bounds = UiRect {
        x: 0,
        y: 0,
        w: size.width as i32,
        h: size.height as i32,
    };
    let b = ui.bounds;
    ui.scissors.clear();
    ui.scissors.push(b);
    ui.scissor_idx = 0;
    ui.layer = 0;
}

// preserve the buffers by zipping together a query with the chunks, spawn new if not enough,
// GC if too many
// most frames should have the same items
fn submit_frame_color_rects(
    mut ui: ResMut<UiState>,
    mut cmd: Commands,
    mut color_rect_q: Query<(&mut RectRequests, &mut UiScissor, EntityId)>,
) {
    let mut color_rects = std::mem::take(&mut ui.color_rects);
    color_rects.sort_unstable_by_key(|r| r.scissor);

    let mut buffers_reused = 0;
    for (g, (rects, sc, _id)) in
        (color_rects.chunk_by(|a, b| a.scissor == b.scissor)).zip(color_rect_q.iter_mut())
    {
        buffers_reused += 1;
        rects.0.clear();
        rects.0.extend_from_slice(g);
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
    }
    for (_, _, id) in color_rect_q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in color_rects
        .chunk_by(|a, b| a.scissor == b.scissor)
        .skip(buffers_reused)
    {
        cmd.spawn().insert_bundle((
            RectRequests(g.iter().copied().collect()),
            UiScissor(ui.scissors[g[0].scissor as usize]),
        ));
    }
}

// preserve the buffers by zipping together a query with the chunks, spawn new if not enough,
// GC if too many
// most frames should have the same items
fn submit_text_rects(
    mut ui: ResMut<UiState>,
    mut cmd: Commands,
    mut text_rect_q: Query<(&mut TextRectRequests, &mut UiScissor, EntityId)>,
) {
    let mut text_rects = std::mem::take(&mut ui.text_rects);
    text_rects.sort_unstable_by_key(|r| r.scissor);

    let mut buffers_reused = 0;
    for (g, (rects, sc, _id)) in
        (text_rects.chunk_by_mut(|a, b| a.scissor == b.scissor)).zip(text_rect_q.iter_mut())
    {
        buffers_reused += 1;
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
        rects.0.clear();
        rects.0.extend(g.iter_mut().map(|x| std::mem::take(x)));
    }
    for (_, _, id) in text_rect_q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in text_rects
        .chunk_by_mut(|a, b| a.scissor == b.scissor)
        .skip(buffers_reused)
    {
        cmd.spawn().insert_bundle((
            UiScissor(ui.scissors[g[0].scissor as usize]),
            TextRectRequests(g.iter_mut().map(|x| std::mem::take(x)).collect()),
        ));
    }
}

pub struct Ui<'a> {
    ui: ResMut<'a, UiState>,
    texture_cache: ResMut<'a, TextTextureCache>,
    shaping_results: ResMut<'a, assets::Assets<ShapingResult>>,
    theme: ResMut<'a, Theme>,
    mouse: Res<'a, MouseInputs>,
    memory: ResMut<'a, UiMemory>,
}

/// Root of the UI used to instantiate UI containers
pub struct UiRoot<'a>(Ui<'a>);

impl<'a> UiRoot<'a> {
    pub fn panel(&mut self, desc: PanelDescriptor, mut contents: impl FnMut(&mut Ui)) {
        let width = desc.width.as_abolute(self.0.ui.bounds.w);
        let height = desc.height.as_abolute(self.0.ui.bounds.h);
        self.0.ui.root_hash = fnv_1a(bytemuck::cast_slice(&[width, height]));

        let old_bounds = self.0.ui.bounds;
        let mut bounds = UiRect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        };

        match desc.horizonal {
            HorizontalAlignment::Left => {}
            HorizontalAlignment::Right => {
                bounds.x = old_bounds.w.saturating_sub(width + 1) as i32;
            }
            HorizontalAlignment::Center => {
                bounds.x = (old_bounds.w / 2).saturating_sub(width / 2) as i32;
            }
        }
        match desc.vertical {
            VerticalAlignment::Top => {}
            VerticalAlignment::Bottom => {
                bounds.y = old_bounds.h.saturating_sub(height + 1) as i32;
            }
            VerticalAlignment::Center => {
                bounds.y = (old_bounds.h / 2).saturating_sub(height / 2) as i32;
            }
        }
        self.0.ui.bounds = bounds;
        let scissor = self.0.ui.scissor_idx;
        self.0.ui.scissor_idx = self.0.ui.scissors.len() as u32;
        self.0.ui.scissors.push(bounds);

        let layer = self.0.ui.layer;
        self.0.ui.layer += 1;
        self.0.color_rect(
            bounds.x,
            bounds.y,
            width,
            height,
            0x04a5e5ff,
            self.0.ui.layer,
        );
        self.0.ui.id_stack.push(0);
        contents(&mut self.0);
        self.0.ui.layer = layer;
        self.0.ui.id_stack.pop();
        self.0.ui.bounds = old_bounds;
        self.0.ui.scissor_idx = scissor;
    }

    pub fn theme(&self) -> &Theme {
        &self.0.theme
    }

    pub fn theme_mut(&mut self) -> &mut ResMut<'a, Theme> {
        &mut self.0.theme
    }
}

unsafe impl<'a> query::WorldQuery<'a> for UiRoot<'a> {
    fn new(db: &'a World, _system_idx: usize) -> Self {
        let ui = ResMut::new(db);
        let texture_cache = ResMut::new(db);
        let text_assets = ResMut::new(db);
        let theme = ResMut::new(db);
        let memory = ResMut::new(db);
        let mouse = Res::new(db);
        Self(Ui {
            ui,
            texture_cache,
            shaping_results: text_assets,
            theme,
            mouse,
            memory,
        })
    }

    fn resources_mut(set: &mut std::collections::HashSet<std::any::TypeId>) {
        set.insert(std::any::TypeId::of::<UiState>());
        set.insert(std::any::TypeId::of::<TextTextureCache>());
        set.insert(std::any::TypeId::of::<assets::Assets<ShapingResult>>());
        set.insert(std::any::TypeId::of::<Theme>());
        set.insert(std::any::TypeId::of::<UiMemory>());
    }

    fn resources_const(set: &mut std::collections::HashSet<std::any::TypeId>) {
        set.insert(std::any::TypeId::of::<MouseInputs>());
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UiScissor(pub UiRect);

// TODO: gc?
#[derive(Default)]
struct UiMemory(pub HashMap<(UiId, TypeId), ErasedMemoryEntry>);

unsafe impl Send for UiMemory {}
unsafe impl Sync for UiMemory {}

pub struct ErasedMemoryEntry {
    inner: *mut u8,
    finalize: fn(&mut ErasedMemoryEntry),
}

impl Drop for ErasedMemoryEntry {
    fn drop(&mut self) {
        (self.finalize)(self);
    }
}

impl ErasedMemoryEntry {
    pub fn new<T>(value: T) -> Self {
        let inner = Box::leak(Box::new(value));
        Self {
            inner: (inner as *mut T).cast(),
            finalize: |resource| unsafe {
                if !resource.inner.is_null() {
                    let _inner: Box<T> = Box::from_raw(resource.inner.cast::<T>());
                    resource.inner = std::ptr::null_mut();
                }
            },
        }
    }

    /// # SAFETY
    /// Must be called with the same type as `new`
    pub unsafe fn as_inner<T>(&self) -> &T {
        &*self.inner.cast()
    }

    /// # SAFETY
    /// Must be called with the same type as `new`
    pub unsafe fn as_inner_mut<T>(&mut self) -> &mut T {
        &mut *self.inner.cast()
    }

    pub unsafe fn into_inner<T>(mut self) -> Box<T> {
        let inner = self.inner;
        self.inner = std::ptr::null_mut();
        Box::from_raw(inner.cast())
    }
}
