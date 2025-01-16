use crate::{
    assets::{self, Assets, AssetsPlugin, Handle, WeakHandle},
    KeyBoardInputs, MouseInputs, Plugin,
};

pub mod color_rect_pipeline;
pub mod rect;
pub mod text;
pub mod text_rect_pipeline;

use std::{any::TypeId, collections::HashMap, ptr::NonNull};

use cecs::{prelude::*, query};
use glam::IVec2;
use text_rect_pipeline::{DrawTextRect, TextRectRequests};
use winit::{
    dpi::PhysicalPosition,
    event::{MouseButton, MouseScrollDelta},
    keyboard::KeyCode,
};

use {
    color_rect_pipeline::{DrawColorRect, RectRequests},
    rect::UiRect,
    text::{OwnedTypeFace, TextDrawResponse},
};

pub type Color = u32;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(color_rect_pipeline::UiColorRectPlugin);
        app.add_plugin(text_rect_pipeline::UiTextRectPlugin);
        app.add_plugin(AssetsPlugin::<OwnedTypeFace>::default());
        app.add_plugin(AssetsPlugin::<ShapingResult>::default());

        app.insert_resource(UiState::new());
        app.insert_resource(TextTextureCache::default());
        app.insert_resource(UiMemory::default());

        if app.get_resource::<Theme>().is_none() {
            app.insert_resource(Theme::default());
        }

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

#[derive(Clone)]
pub struct ShapeKey {
    pub text: String,
    pub size: u32,
    pub font: WeakHandle<OwnedTypeFace>,
    // TODO: include font handle, shaping info etc
}

impl Eq for ShapeKey {}

impl PartialEq for ShapeKey {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.size == other.size && self.font.id() == other.font.id()
    }
}

impl std::hash::Hash for ShapeKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.size.hash(state);
        self.font.id().hash(state);
    }
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

    /// layers go from back to front
    layer: u16,

    bounding_boxes: HashMap<UiId, UiRect>,

    rect_history: Vec<UiRect>,

    /// hash of the current tree root
    root_hash: u32,

    layout_dir: LayoutDirection,

    /// TODO: gc?
    windows: HashMap<String, WindowState>,
    fallback_font: OwnedTypeFace,
}

#[derive(Clone)]
pub struct Theme {
    pub primary_color: Color,
    pub secondary_color: Color,
    pub button_hovered: Color,
    pub button_pressed: Color,
    pub text_padding: u16,
    pub font_size: u16,
    pub padding: u16,
    pub scroll_bar_size: u16,
    pub window_title_height: u8,
    pub font: Handle<OwnedTypeFace>,
    pub window_padding: u8,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            primary_color: 0xcdd6f4ff,
            secondary_color: 0x212224ff,
            button_hovered: 0x45475aff,
            button_pressed: 0x585b70ff,
            text_padding: 5,
            font_size: 12,
            padding: 5,
            scroll_bar_size: 12,
            window_title_height: 24,
            font: Default::default(),
            window_padding: 4,
        }
    }
}

impl UiState {
    pub fn new() -> Self {
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
            bounding_boxes: Default::default(),
            rect_history: Default::default(),
            root_hash: 0,
            layout_dir: LayoutDirection::TopDown,
            windows: Default::default(),
            fallback_font: text::parse_font(
                include_bytes!("./ui/Roboto-Regular.ttf")
                    .to_vec()
                    .into_boxed_slice(),
                0,
            )
            .unwrap(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutDirection {
    TopDown,
    LeftRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCoord {
    Absolute(i32),
    Percent(i8),
}

impl Default for UiCoord {
    fn default() -> Self {
        Self::Absolute(0)
    }
}

impl From<i32> for UiCoord {
    fn from(value: i32) -> Self {
        Self::Absolute(value)
    }
}

impl UiCoord {
    pub fn as_abolute(self, max: i32) -> i32 {
        match self {
            UiCoord::Absolute(x) => x,
            UiCoord::Percent(p) => {
                let p = p as f64 / 100.0;
                let x = max as f64 * p;
                x as i32
            }
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct PanelDescriptor {
    pub width: UiCoord,
    pub height: UiCoord,
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
    /// returns the last scissor_idx
    pub fn push_scissor(&mut self, scissor_bounds: UiRect) -> u32 {
        let res = self.ui.scissor_idx;
        self.ui.scissor_idx = self.ui.scissors.len() as u32;
        self.ui.scissors.push(scissor_bounds);
        res
    }

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
    pub fn is_anything_active(&self) -> bool {
        self.ui.active != UiId::SENTINEL
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
        let mouse = self.mouse.cursor_position;

        if let Some(scissor) = self.ui.scissors.get(self.ui.scissor_idx as usize) {
            if !scissor.contains_point(mouse.x as i32, mouse.y as i32) {
                return false;
            }
        }

        let Some(bbox) = self.ui.bounding_boxes.get(&id) else {
            return false;
        };
        bbox.contains_point(mouse.x as i32, mouse.y as i32)
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
        ///////////////////////
        contents(self);
        ///////////////////////
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
        ///////////////////////
        contents(self);
        ///////////////////////
        self.ui.id_stack.pop();
        self.ui.layout_dir = layout;
        self.ui.bounds = bounds;
        self.submit_rect_group(history_start);
    }

    /// submit a new rect that contains all rects submitted beginning at history_start index
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
        ///////////////////////
        contents(&mut cols);
        ///////////////////////

        self.ui.id_stack.pop();
        self.ui.bounds = bounds;
        self.submit_rect_group(history_start);
    }

    pub fn color_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: Color,
        layer: u16,
    ) {
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
        color: Color,
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
                font: self.theme.font.downgrade(),
            })
            .or_insert_with(|| {
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line);
                let font = if self.fonts.contains(self.theme.font.id()) {
                    self.fonts.get(&self.theme.font)
                } else {
                    &self.ui.fallback_font
                };

                let glyphs = rustybuzz::shape(font.face(), &[], buffer);
                let pic = text::draw_glyph_buffer(font.face(), &glyphs, size).unwrap();

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

        ///////////////////////
        contents(self);
        ///////////////////////

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
            let (handle, e) =
                self.shape_and_draw_line(line.to_owned(), self.theme.font_size as u32);
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
            let (handle, e) =
                self.shape_and_draw_line(line.to_owned(), self.theme.font_size as u32);
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

    pub fn get_memory_or_default<T: Default + 'static>(&mut self, id: UiId) -> &mut T {
        self.get_memory_or_insert(id, Default::default)
    }

    pub fn get_memory_or_insert<T: 'static>(&mut self, id: UiId, f: impl FnOnce() -> T) -> &mut T {
        let key = self.memory_key::<T>(id);
        let res = self
            .memory
            .0
            .entry(key)
            .or_insert_with(|| ErasedMemoryEntry::new(f()));
        unsafe { res.as_inner_mut() }
    }

    pub fn get_memory<T: 'static>(&self, id: UiId) -> Option<&T> {
        let key = self.memory_key::<T>(id);
        self.memory.0.get(&key).map(|res| unsafe { res.as_inner() })
    }

    pub fn get_memory_mut<T: 'static>(&mut self, id: UiId) -> Option<&mut T> {
        let key = self.memory_key::<T>(id);
        self.memory
            .0
            .get_mut(&key)
            .map(|res| unsafe { res.as_inner_mut() })
    }

    pub fn insert_memory<T: 'static>(&mut self, id: UiId, item: T) {
        let key = self.memory_key::<T>(id);
        self.memory.0.insert(key, ErasedMemoryEntry::new(item));
    }

    pub fn remove_memory<T: 'static>(&mut self, id: UiId) -> Option<Box<T>> {
        let key = self.memory_key::<T>(id);
        self.memory
            .0
            .remove(&key)
            .map(|res| unsafe { res.into_inner() })
    }

    fn memory_key<T: 'static>(&self, id: UiId) -> (UiId, TypeId) {
        (id, TypeId::of::<T>())
    }

    fn vertical_scroll_bar(
        &mut self,
        scissor_bounds: &UiRect,
        scroll_bar_width: i32,
        layer: u16,
        parent_state: &mut ScrollState,
    ) {
        self.begin_widget();

        // bar
        let bounds = UiRect {
            x: scissor_bounds.x_end().saturating_sub(scroll_bar_width),
            y: scissor_bounds.y,
            w: scroll_bar_width,
            h: scissor_bounds.h,
        };
        self.color_rect(bounds.x, bounds.y, bounds.w, bounds.h, 0xFF0000FF, layer);
        let id = self.current_id();
        self.ui.bounding_boxes.insert(id, bounds);

        // pip
        let t = parent_state.ty;
        let id = self.current_id();
        let active = self.is_active(id);
        let contains_mouse = self.contains_mouse(id);
        let mut y = scissor_bounds.y - (scissor_bounds.h as f32 * t) as i32;
        if active {
            if self.mouse_up() {
                self.set_not_active(id);
            } else {
                let coord = self.mouse.cursor_position.y as i32;
                let coord =
                    coord.clamp(scissor_bounds.y, scissor_bounds.y_end() - scroll_bar_width);
                y = coord as i32;
                parent_state.ty = -(y - scissor_bounds.y) as f32 / scissor_bounds.h as f32;
            }
        } else if self.is_hovered(id) {
            if !contains_mouse {
                self.set_not_hovered(id);
            } else if self.mouse_down() {
                self.set_active(id);
            }
        }
        if contains_mouse {
            self.set_hovered(id);
        }
        let control_box = UiRect {
            x: scissor_bounds.x_end().saturating_sub(scroll_bar_width),
            y,
            w: scroll_bar_width,
            h: scroll_bar_width,
        };
        self.color_rect(
            control_box.x,
            control_box.y,
            control_box.w,
            control_box.h,
            0xFF0AA0FF,
            layer + 1,
        );
        self.ui.bounding_boxes.insert(id, control_box);
    }

    fn horizontal_scroll_bar(
        &mut self,
        scissor_bounds: &UiRect,
        scroll_bar_height: i32,
        layer: u16,
        parent_state: &mut ScrollState,
    ) {
        self.begin_widget();

        // bar
        let bounds = UiRect {
            x: scissor_bounds.x,
            y: scissor_bounds.y_end().saturating_sub(scroll_bar_height),
            w: scissor_bounds.w,
            h: scroll_bar_height,
        };
        self.color_rect(bounds.x, bounds.y, bounds.w, bounds.h, 0xaaFF00FF, layer);
        let id = self.current_id();
        self.ui.bounding_boxes.insert(id, bounds);

        // pip
        let t = parent_state.tx;
        let id = self.current_id();
        let active = self.is_active(id);
        let contains_mouse = self.contains_mouse(id);
        let mut x = scissor_bounds.x - (scissor_bounds.w as f32 * t) as i32;
        if active {
            if self.mouse_up() {
                self.set_not_active(id);
            } else {
                let coord = self.mouse.cursor_position.x as i32;
                let coord =
                    coord.clamp(scissor_bounds.x, scissor_bounds.x_end() - scroll_bar_height);
                x = coord as i32;
                parent_state.tx = -(x - scissor_bounds.x) as f32 / scissor_bounds.w as f32;
            }
        } else if self.is_hovered(id) {
            if !contains_mouse {
                self.set_not_hovered(id);
            } else if self.mouse_down() {
                self.set_active(id);
            }
        }
        if contains_mouse {
            self.set_hovered(id);
        }
        let control_box = UiRect {
            y: scissor_bounds.y_end().saturating_sub(scroll_bar_height),
            x,
            w: scroll_bar_height,
            h: scroll_bar_height,
        };
        self.color_rect(
            control_box.x,
            control_box.y,
            control_box.w,
            control_box.h,
            0xFF0AA0FF,
            layer + 1,
        );
        self.ui.bounding_boxes.insert(id, control_box);
    }

    pub fn scroll_area(&mut self, desc: ScrollDescriptor, mut contents: impl FnMut(&mut Self)) {
        self.begin_widget();
        let id = self.current_id();
        let width = desc.width.unwrap_or(UiCoord::Percent(100));
        let height = desc.height.unwrap_or(UiCoord::Percent(100));
        let width = width.as_abolute(self.ui.bounds.w);
        let height = height.as_abolute(self.ui.bounds.h);
        let mut state = *self.get_memory_or_default::<ScrollState>(id);

        let line_height = self.theme.font_size + self.theme.text_padding;

        'scroll_handler: {
            if self.contains_mouse(id) {
                let mut dt = 0.0;
                for ds in self.mouse.scroll.iter() {
                    match ds {
                        MouseScrollDelta::LineDelta(_, dy) => {
                            dt += *dy / line_height as f32;
                        }
                        MouseScrollDelta::PixelDelta(physical_position) => {
                            dt += physical_position.y as f32;
                        }
                    }
                }
                if dt != 0.0 {
                    let t;
                    if self.keyboard.pressed.contains(&KeyCode::ShiftLeft)
                        || self.keyboard.pressed.contains(&KeyCode::ShiftRight)
                    {
                        if desc.width.is_none() {
                            break 'scroll_handler;
                        }
                        t = &mut state.tx;
                    } else {
                        if desc.height.is_none() {
                            break 'scroll_handler;
                        }
                        t = &mut state.ty;
                    }
                    *t += dt;
                    // TODO: animation at edge?
                    // TODO: the min needs work, doesn't work as intended
                    *t = t.clamp(-1.0 + 1.0 / (line_height as f32), 0.0);
                }
            }
        }
        let offset_x = state.tx * state.content_width as f32;
        let offset_y = state.ty * state.content_height as f32;
        self.insert_memory(id, state);

        let old_bounds = self.ui.bounds;
        let scissor_bounds = UiRect {
            x: old_bounds.x,
            y: old_bounds.y,
            w: width,
            h: height,
        };
        let mut bounds = scissor_bounds;
        bounds.x += offset_x as i32;
        bounds.y += offset_y as i32;
        if desc.width.is_some() {
            bounds.w = bounds.w.saturating_sub(self.theme.scroll_bar_size as i32);
        }
        if desc.height.is_some() {
            bounds.h = bounds.h.saturating_sub(self.theme.scroll_bar_size as i32);
        }

        self.ui.bounds = bounds;
        let scissor_idx = self.ui.scissor_idx;
        self.ui.scissor_idx = self.ui.scissors.len() as u32;
        self.ui.scissors.push(scissor_bounds);

        let layer = self.ui.layer;
        self.ui.layer += 1;
        self.color_rect(bounds.x, bounds.y, width, height, 0x04a5e5ff, self.ui.layer);
        self.ui.id_stack.push(0);
        let history_start = self.ui.rect_history.len();
        ///////////////////////
        contents(self);
        ///////////////////////
        let last_id = self.ui.id_stack.pop().unwrap();
        let children_bounds = self.history_bounding_rect(history_start);

        let state = self.get_memory_mut::<ScrollState>(id).unwrap();

        state.content_width = children_bounds.w;
        state.content_height = children_bounds.h;
        let mut state = *state;

        let scroll_bar_size = self.theme.scroll_bar_size as i32;
        self.ui.id_stack.push(last_id);
        if desc.width.is_some() {
            let mut scissor_bounds = scissor_bounds;
            if desc.height.is_some() {
                // prevent overlap
                scissor_bounds.w -= scroll_bar_size;
            }
            self.horizontal_scroll_bar(&scissor_bounds, scroll_bar_size, layer + 2, &mut state);
        }
        if desc.height.is_some() {
            self.vertical_scroll_bar(&scissor_bounds, scroll_bar_size, layer + 2, &mut state);
        }
        self.ui.id_stack.pop();
        self.insert_memory(id, state);
        self.submit_rect(id, scissor_bounds);

        self.ui.layer = layer;
        self.ui.bounds = old_bounds;
        self.ui.scissor_idx = scissor_idx;
    }

    fn history_bounding_rect(&self, history_start: usize) -> UiRect {
        bounding_rect(&self.ui.rect_history[history_start..])
    }

    pub fn drag_source(&mut self, mut contents: impl FnMut(&mut Self)) -> DragResponse {
        self.begin_widget();
        let id = self.current_id();
        let old_bounds = self.ui.bounds;

        let mut state = self
            .remove_memory::<DragState>(id)
            .map(|x| *x)
            .unwrap_or_default();
        let mut is_being_dragged = false;
        if self.is_active(id) {
            // mark as dragged even if it was just released,
            // otherwise the parent bounds will end
            // up weird for 1 frame
            is_being_dragged = true;
            if self.mouse_up() {
                self.set_not_active(id);
            } else {
                let drag_anchor = state.drag_anchor;
                let drag_start = state.drag_start;

                let offset = IVec2::new(
                    (self.mouse.cursor_position.x - drag_start.x) as i32,
                    (self.mouse.cursor_position.y - drag_start.y) as i32,
                );

                state.pos = drag_anchor + offset;
            }
        } else {
            state.pos = IVec2::new(old_bounds.x, old_bounds.y);
            if !self.is_anything_active() && self.contains_mouse(id) && self.mouse_down() {
                is_being_dragged = true;
                state.drag_start = self.mouse.cursor_position;
                self.set_active(id);
            }
        }

        let history = std::mem::take(&mut self.ui.rect_history);
        self.ui.bounds.x = state.pos.x;
        self.ui.bounds.y = state.pos.y;
        self.ui.bounds.w = state.size.x;
        self.ui.bounds.h = state.size.y;
        let last_scissor = self.ui.scissor_idx;
        self.push_scissor(self.ui.bounds);
        self.ui.layer += 1;
        self.ui.id_stack.push(0);
        ///////////////////////
        contents(self);
        ///////////////////////
        self.ui.layer -= 1;
        self.ui.id_stack.pop();
        self.ui.bounds = old_bounds;
        self.ui.scissor_idx = last_scissor;
        let child_history = std::mem::replace(&mut self.ui.rect_history, history);
        let mut content_bounds = bounding_rect(&child_history);
        if is_being_dragged {
            content_bounds.x = state.drag_anchor.x;
            content_bounds.y = state.drag_anchor.y;
            self.ui.rect_history.push(content_bounds);
        } else {
            self.ui.rect_history.extend_from_slice(&child_history);
            state.drag_anchor = IVec2::new(content_bounds.x, content_bounds.y);
            state.pos = state.drag_anchor;
        }

        self.submit_rect(id, content_bounds);
        state.size = IVec2::new(content_bounds.w, content_bounds.h);

        self.insert_memory(id, state);

        DragResponse { is_being_dragged }
    }
}

#[derive(Debug, Default)]
struct DragState {
    pub drag_start: PhysicalPosition<f64>,
    pub drag_anchor: IVec2,
    pub pos: IVec2,
    pub size: IVec2,
}

#[derive(Debug, Clone, Copy)]
pub struct DragResponse {
    pub is_being_dragged: bool,
}

/// If a field is None, then the area does not scroll on that axis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollDescriptor {
    pub width: Option<UiCoord>,
    pub height: Option<UiCoord>,
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

        ///////////////////////
        contents(ctx);
        ///////////////////////

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
    keyboard: Res<'a, KeyBoardInputs>,
    memory: ResMut<'a, UiMemory>,
    fonts: Res<'a, Assets<OwnedTypeFace>>,
}

/// Root of the UI used to instantiate UI containers
pub struct UiRoot<'a>(Ui<'a>);

#[derive(Debug)]
struct WindowState {
    pos: IVec2,
    drag_anchor: IVec2,
    drag_start: PhysicalPosition<f64>,
    content_size: IVec2,
    size: IVec2,
}

pub struct WindowDescriptor<'a> {
    pub name: &'a str,
    pub show_title: bool,
    /// Initial window position
    pub pos: Option<IVec2>,
    /// Initial window size
    pub size: Option<IVec2>,
}

impl<'a> Default for WindowDescriptor<'a> {
    fn default() -> Self {
        Self {
            name: "some window",
            show_title: true,
            pos: None,
            size: None,
        }
    }
}

const WINDOW_LAYER: u16 = 100;

impl<'a> UiRoot<'a> {
    pub fn window(&mut self, desc: WindowDescriptor, mut contents: impl FnMut(&mut Ui)) {
        let state: &mut WindowState = self
            .0
            .ui
            .windows
            .entry(desc.name.to_owned())
            .or_insert_with(|| {
                // TODO: allocate window
                WindowState {
                    pos: desc.pos.unwrap_or(IVec2::new(500, 500)),
                    size: desc.size.unwrap_or(IVec2::splat(100)),
                    drag_anchor: Default::default(),
                    drag_start: Default::default(),
                    content_size: IVec2::ZERO,
                }
            });

        let padding = self.0.theme.window_padding as i32;
        let width = state.size.x;
        let height = state.size.y - self.0.theme.window_title_height as i32;
        let bounds = UiRect {
            x: state.pos.x,
            y: state.pos.y + self.0.theme.window_title_height as i32,
            w: width,
            h: height,
        };
        let title_bounds = UiRect {
            x: state.pos.x,
            y: state.pos.y,
            w: width + 2 * padding,
            h: self.0.theme.window_title_height as i32,
        };

        self.0.ui.root_hash = fnv_1a(desc.name.as_bytes());
        let old_bounds = self.0.ui.bounds;
        let scissor = self.0.ui.scissor_idx;

        let layer = self.0.ui.layer;
        self.0.ui.layer = WINDOW_LAYER;
        // window background
        self.0.color_rect(
            bounds.x,
            bounds.y,
            width + padding * 2,
            height + padding * 2,
            0x0395d5ff,
            WINDOW_LAYER,
        );
        self.0.ui.id_stack.push(0);
        ///////////////////////
        // Title
        {
            self.0.ui.bounds = title_bounds;
            self.0.push_scissor(title_bounds);
            self.0.label(desc.name);
            self.0.begin_widget();
            let title_id = self.0.current_id();
            if self.0.is_active(title_id) {
                if self.0.mouse_up() {
                    self.0.set_not_active(title_id);
                }
                let state: &mut WindowState = self.0.ui.windows.get_mut(desc.name).unwrap();

                let drag_anchor = state.drag_anchor;
                let drag_start = state.drag_start;

                let offset = IVec2::new(
                    (self.0.mouse.cursor_position.x - drag_start.x) as i32,
                    (self.0.mouse.cursor_position.y - drag_start.y) as i32,
                );

                state.pos = drag_anchor + offset;
            } else {
                if self.0.contains_mouse(title_id) && self.0.mouse_down() {
                    let state: &mut WindowState = self.0.ui.windows.get_mut(desc.name).unwrap();
                    state.drag_start = self.0.mouse.cursor_position;
                    state.drag_anchor = state.pos;
                    self.0.set_active(title_id);
                }
            }
            self.0.submit_rect(title_id, title_bounds);
            self.0.color_rect(
                title_bounds.x,
                title_bounds.y,
                title_bounds.w,
                title_bounds.h,
                0x00ffffff,
                WINDOW_LAYER,
            );
        }
        ///////////////////////
        ///////////////////////
        // Content
        let history_start = self.0.ui.rect_history.len();
        {
            self.0.push_scissor(bounds);
            let mut bounds = bounds;
            bounds.x += padding;
            bounds.y += padding;
            bounds.w -= 2 * padding;
            bounds.h -= 2 * padding;
            self.0.ui.bounds = bounds;
            self.0.ui.layer = WINDOW_LAYER + 2;
            self.0.begin_widget();
            contents(&mut self.0);
        }
        ///////////////////////
        self.0.ui.layer = layer;
        self.0.ui.id_stack.pop();
        self.0.ui.bounds = old_bounds;
        self.0.ui.scissor_idx = scissor;

        let r = self.0.history_bounding_rect(history_start);

        let state: &mut WindowState = self.0.ui.windows.get_mut(desc.name).unwrap();
        state.content_size = IVec2::new(r.w, r.h);
        state.size = state.content_size + 2 * IVec2::splat(padding);
        state.size.y = (state.size.y).max(5) + self.0.theme.window_title_height as i32;
    }

    pub fn panel(&mut self, desc: PanelDescriptor, mut contents: impl FnMut(&mut Ui)) {
        let width = desc.width.as_abolute(self.0.ui.bounds.w);
        let height = desc.height.as_abolute(self.0.ui.bounds.h);

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
        self.0.ui.root_hash = fnv_1a(bytemuck::cast_slice(&[bounds.x, bounds.y, width, height]));
        self.0.ui.bounds = bounds;
        let scissor = self.0.push_scissor(bounds);

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
        ///////////////////////
        contents(&mut self.0);
        ///////////////////////
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

    pub fn with_theme(&mut self, theme: Theme, mut contents: impl FnMut(&mut Self)) {
        let t = std::mem::replace(&mut *self.0.theme, theme);

        ///////////////////////
        contents(self);
        ///////////////////////

        *self.0.theme = t;
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
        let keyboard = Res::new(db);
        let fonts = Res::new(db);
        Self(Ui {
            ui,
            texture_cache,
            shaping_results: text_assets,
            theme,
            mouse,
            keyboard,
            memory,
            fonts,
        })
    }

    fn resources_mut(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<UiState>());
        set.insert(TypeId::of::<TextTextureCache>());
        set.insert(TypeId::of::<Assets<ShapingResult>>());
        set.insert(TypeId::of::<Theme>());
        set.insert(TypeId::of::<UiMemory>());
    }

    fn resources_const(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<MouseInputs>());
        set.insert(TypeId::of::<KeyBoardInputs>());
        set.insert(TypeId::of::<Assets<OwnedTypeFace>>());
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

#[derive(Default, Debug, Clone, Copy)]
struct ScrollState {
    /// goes from -1 to 0
    pub tx: f32,
    pub ty: f32,
    pub content_width: i32,
    pub content_height: i32,
}

fn bounding_rect(history: &[UiRect]) -> UiRect {
    if history.is_empty() {
        return Default::default();
    }

    let mut max_x = std::i32::MIN;
    let mut min_x = std::i32::MAX;
    let mut max_y = std::i32::MIN;
    let mut min_y = std::i32::MAX;
    for r in history {
        min_x = min_x.min(r.x);
        max_x = max_x.max(r.x_end());

        min_y = min_y.min(r.y);
        max_y = max_y.max(r.y_end());
    }

    let w = max_x - min_x;
    let h = max_y - min_y;

    UiRect {
        x: min_x + w / 2,
        y: min_y + h / 2,
        w,
        h,
    }
}
