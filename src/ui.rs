use crate::{
    assets::{self, Assets, AssetsPlugin, Handle, WeakHandle},
    DeltaTime, KeyBoardInputs, MouseInputs, Plugin, Timer,
};

pub mod color_rect_pipeline;
pub mod rect;
pub mod text;
pub mod text_rect_pipeline;

use std::{any::TypeId, collections::HashMap, ptr::NonNull, time::Duration};

use cecs::{prelude::*, query};
use glam::IVec2;
use text_rect_pipeline::{DrawTextRect, TextRectRequests};
use tracing::debug;
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
        app.insert_resource(UiIds::default());
        app.insert_resource(NextUiIds(UiIds::default()));
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
                .add_system(submit_frame_text_rects)
                .add_system(update_ids);
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

fn update_ids(mut lhs: ResMut<UiIds>, rhs: Res<NextUiIds>) {
    *lhs = rhs.0;
}

#[derive(Default, Debug, Clone, Copy)]
pub struct UiIds {
    hovered: UiId,
    active: UiId,
    dragged: UiId,
}
pub struct NextUiIds(pub UiIds);

/// UI context object. Use this to builder your user interface
pub struct UiState {
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

    window_allocator: WindowAllocator,
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
            window_allocator: WindowAllocator {
                next: IVec2::new(100, 100),
            },
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
        self.next_ids.0.hovered = id;
    }

    #[inline]
    fn set_active(&mut self, id: UiId) {
        self.next_ids.0.active = id;
    }

    #[inline]
    pub fn is_anything_dragged(&self) -> bool {
        self.ids.dragged != UiId::SENTINEL
    }

    pub fn clear_active(&mut self) {
        self.next_ids.0.active = UiId::SENTINEL;
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
    pub fn is_active(&self, id: UiId) -> bool {
        self.ids.active == id
    }

    #[inline]
    pub fn is_anything_active(&self) -> bool {
        self.ids.active != UiId::SENTINEL
    }

    #[inline]
    pub fn is_hovered(&self, id: UiId) -> bool {
        self.ids.hovered == id
    }

    #[inline]
    pub fn mouse_up(&self) -> bool {
        self.mouse.just_released.contains(&MouseButton::Left)
    }

    #[inline]
    pub fn mouse_down(&self) -> bool {
        self.mouse.pressed.contains(&MouseButton::Left)
    }

    #[inline]
    pub fn contains_mouse(&self, id: UiId) -> bool {
        let Some(bbox) = self.widget_bounds(id) else {
            return false;
        };
        let mouse = self.mouse.cursor_position;
        bbox.contains_point(mouse.x as i32, mouse.y as i32)
    }

    pub fn widget_bounds(&self, id: UiId) -> Option<UiRect> {
        let mut bbox = *self.ui.bounding_boxes.get(&id)?;
        if let Some(scissor) = self.ui.scissors.get(self.ui.scissor_idx as usize) {
            bbox = bbox.intersection(*scissor)?;
        }
        Some(bbox)
    }

    #[inline]
    fn set_not_active(&mut self, id: UiId) {
        if self.ids.active == id {
            self.next_ids.0.active = UiId::SENTINEL;
        }
    }

    #[inline]
    fn set_not_hovered(&mut self, id: UiId) {
        if self.ids.hovered == id {
            self.next_ids.0.hovered = UiId::SENTINEL;
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
        let width = (bounds.width() / cols + 1) as i32;

        let dims = (0..cols as i32)
            .map(|i| [bounds.min_x + i * width, bounds.min_x + (i + 1) * width])
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

    pub fn color_rect_from_rect(&mut self, rect: UiRect, color: Color, layer: u16) {
        self.color_rect(
            rect.min_x,
            rect.min_y,
            rect.width(),
            rect.height(),
            color,
            layer,
        );
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
        self.ui
            .rect_history
            .push(UiRect::from_pos_size(x, y, width, height));
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
            min_x: x,
            min_y: y,
            max_x: x + width,
            max_y: y + height,
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

    pub fn get_current_font(&self) -> &OwnedTypeFace {
        if self.fonts.contains(self.theme.font.id()) {
            self.fonts.get(&self.theme.font)
        } else {
            &self.ui.fallback_font
        }
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
        let x = self.ui.bounds.min_x;
        let y = self.ui.bounds.min_y;
        let padding = self.theme.padding as i32;
        let [x, y] = [x + padding, y + padding];
        let mut text_y = y;
        let mut line_height = 0;
        for line in label.split('\n') {
            if line.is_empty() {
                text_y += line_height;
                continue;
            }
            let (handle, e) =
                self.shape_and_draw_line(line.to_owned(), self.theme.font_size as u32);
            let pic = &e.texture;
            let line_width = pic.width() as i32;
            line_height = pic.height() as i32;
            w = w.max(line_width);
            h += line_height;

            self.text_rect(
                x,
                text_y,
                line_width,
                line_height,
                self.theme.secondary_color,
                layer + 1,
                handle,
            );
            text_y += line_height;
        }
        let rect = UiRect {
            min_x: x,
            min_y: y,
            max_x: x + w,
            max_y: y + h,
        };
        self.submit_rect(id, rect);
        Response {
            hovered: self.ids.hovered == id,
            active: self.ids.active == id,
            inner: (),
            rect,
        }
    }

    /// When a widget has been completed, submit its bounding rectangle
    fn submit_rect(&mut self, id: UiId, rect: UiRect) {
        let padding = self.theme.padding as i32;
        match self.ui.layout_dir {
            LayoutDirection::TopDown => {
                let dy = rect.height() + 2 * padding;
                self.ui.bounds.min_y += dy;
            }
            LayoutDirection::LeftRight => {
                let dx = rect.width() + 2 * padding;
                self.ui.bounds.min_x += dx;
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
        if contains_mouse && !self.is_anything_active() {
            self.set_hovered(id);
        }

        // shape the text
        let mut w = 0;
        let mut h = 0;
        let x = self.ui.bounds.min_x;
        let y = self.ui.bounds.min_y;
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

        let rect = UiRect {
            min_x: x,
            min_y: y,
            max_x: x + w,
            max_y: y + h,
        };
        self.submit_rect(id, rect);
        ButtonResponse {
            hovered: self.ids.hovered == id,
            active: self.ids.active == id,
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
            min_x: scissor_bounds.max_x - scroll_bar_width,
            min_y: scissor_bounds.min_y,
            max_x: scissor_bounds.max_x,
            max_y: scissor_bounds.max_y,
        };
        self.color_rect_from_rect(bounds, 0xFF0000FF, layer);
        let id = self.current_id();
        self.ui.bounding_boxes.insert(id, bounds);

        // pip
        let t = parent_state.ty;
        let id = self.current_id();
        let active = self.is_active(id);
        let contains_mouse = self.contains_mouse(id);
        let mut y = scissor_bounds.min_y + (scissor_bounds.height() as f32 * t) as i32;
        if active {
            if self.mouse_up() {
                self.set_not_active(id);
            } else {
                let coord = self.mouse.cursor_position.y as i32;
                let coord = coord.clamp(
                    scissor_bounds.min_y,
                    scissor_bounds.max_y - scroll_bar_width,
                );
                y = coord as i32;
                parent_state.ty = (y - scissor_bounds.min_y) as f32
                    / (scissor_bounds.height() - scroll_bar_width) as f32;
            }
        } else if self.is_hovered(id) {
            if !contains_mouse {
                self.set_not_hovered(id);
            } else if self.mouse_down() {
                self.set_active(id);
            }
        }
        if !self.is_anything_active() && contains_mouse {
            self.set_hovered(id);
        }
        let x = scissor_bounds.max_x.saturating_sub(scroll_bar_width);
        let control_box = UiRect {
            min_x: x,
            min_y: y,
            max_x: scissor_bounds.max_x,
            max_y: y + scroll_bar_width,
        };
        self.color_rect_from_rect(control_box, 0xFF0AA0FF, layer + 1);
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
            min_x: scissor_bounds.min_x,
            min_y: scissor_bounds.max_y - scroll_bar_height,
            max_x: scissor_bounds.max_x,
            max_y: scissor_bounds.max_y,
        };
        self.color_rect_from_rect(bounds, 0xaaFF00FF, layer);
        let id = self.current_id();
        self.ui.bounding_boxes.insert(id, bounds);

        // pip
        let t = parent_state.tx;
        let id = self.current_id();
        let contains_mouse = self.contains_mouse(id);
        let mut x = scissor_bounds.min_x + (scissor_bounds.width() as f32 * t) as i32;
        if self.is_active(id) {
            if self.mouse_up() {
                self.set_not_active(id);
            } else {
                let coord = self.mouse.cursor_position.x as i32;
                let coord = coord.clamp(
                    scissor_bounds.min_x,
                    scissor_bounds.max_x - scroll_bar_height,
                );
                x = coord as i32;
                parent_state.tx = (x - scissor_bounds.min_x) as f32
                    / (scissor_bounds.width() - scroll_bar_height) as f32;
            }
        } else if self.is_hovered(id) {
            if !contains_mouse {
                self.set_not_hovered(id);
            } else if self.mouse_down() {
                self.set_active(id);
            }
        }
        if contains_mouse && !self.is_anything_active() {
            self.set_hovered(id);
        }
        let control_box = UiRect {
            min_x: x,
            min_y: scissor_bounds.max_y - scroll_bar_height,
            max_x: x + scroll_bar_height,
            max_y: scissor_bounds.max_y,
        };
        self.color_rect_from_rect(control_box, 0xFF0AA0FF, layer + 1);
        self.ui.bounding_boxes.insert(id, control_box);
    }

    pub fn scroll_area(&mut self, desc: ScrollDescriptor, mut contents: impl FnMut(&mut Self)) {
        self.begin_widget();
        let id = self.current_id();
        let width = desc
            .width
            .unwrap_or(UiCoord::Percent(100))
            .as_abolute(self.ui.bounds.width());
        let height = desc
            .height
            .unwrap_or(UiCoord::Percent(100))
            .as_abolute(self.ui.bounds.height());
        let mut state = *self.get_memory_or_default::<ScrollState>(id);

        let line_height = self.theme.font_size + self.theme.text_padding;

        'scroll_handler: {
            if self.contains_mouse(id) {
                let mut dt = 0.0;
                for ds in self.mouse.scroll.iter() {
                    match ds {
                        MouseScrollDelta::LineDelta(_, dy) => {
                            dt -= *dy / line_height as f32;
                        }
                        MouseScrollDelta::PixelDelta(physical_position) => {
                            dt -= physical_position.y as f32;
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
                    *t = t.clamp(0.0, 1.0 - 1.0 / (line_height as f32));
                }
            }
        }
        let offset_x = state.tx * state.content_width as f32;
        let offset_y = state.ty * state.content_height as f32;
        self.insert_memory(id, state);

        let old_bounds = self.ui.bounds;
        let mut scissor_bounds = old_bounds;
        scissor_bounds.resize_w(width);
        scissor_bounds.resize_h(height);
        let scissor_bounds = scissor_bounds;

        let mut bounds = scissor_bounds;
        bounds.offset_x(-offset_x as i32);
        bounds.offset_y(-offset_y as i32);
        if desc.width.is_some() {
            bounds.shrink_x(self.theme.scroll_bar_size as i32);
        }
        if desc.height.is_some() {
            bounds.shrink_y(self.theme.scroll_bar_size as i32);
        }

        self.ui.bounds = bounds;
        let scissor_idx = self.push_scissor(scissor_bounds);

        let layer = self.push_layer();
        self.color_rect(
            bounds.min_x,
            bounds.min_y,
            width,
            height,
            0x04a5e5ff,
            self.ui.layer,
        );
        self.ui.id_stack.push(0);
        let history_start = self.ui.rect_history.len();
        ///////////////////////
        contents(self);
        ///////////////////////
        let last_id = self.ui.id_stack.pop().unwrap();
        let children_bounds = self.history_bounding_rect(history_start);

        let state = self.get_memory_mut::<ScrollState>(id).unwrap();

        state.content_width = children_bounds.width();
        state.content_height = children_bounds.height();
        let mut state = *state;

        let scroll_bar_size = self.theme.scroll_bar_size as i32;
        self.ui.id_stack.push(last_id);
        if desc.width.is_some() {
            let mut scissor_bounds = scissor_bounds;
            if desc.height.is_some() {
                // prevent overlap
                scissor_bounds.max_x -= scroll_bar_size;
            }
            self.horizontal_scroll_bar(&scissor_bounds, scroll_bar_size, layer + 2, &mut state);
        }
        if desc.height.is_some() {
            self.vertical_scroll_bar(&scissor_bounds, scroll_bar_size, layer + 2, &mut state);
        }
        self.ui.id_stack.pop();
        self.insert_memory(id, state);
        self.ui.bounds = old_bounds;
        self.submit_rect(id, scissor_bounds);

        self.ui.layer = layer;
        self.ui.scissor_idx = scissor_idx;
    }

    fn history_bounding_rect(&self, history_start: usize) -> UiRect {
        bounding_rect(&self.ui.rect_history[history_start..])
    }

    pub fn empty(&mut self, width: i32, height: i32) -> Response<()> {
        let mut bounds = self.ui.bounds;
        bounds.resize_w(width);
        bounds.resize_h(height);
        // TODO: layout

        self.begin_widget();
        let id = self.current_id();
        self.submit_rect(id, bounds);
        self.ui.rect_history.push(bounds);
        Response {
            hovered: self.is_hovered(id),
            active: self.is_active(id),
            rect: bounds,
            inner: (),
        }
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
                self.next_ids.0.dragged = UiId::SENTINEL;
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
            state.pos = IVec2::new(old_bounds.min_x, old_bounds.min_y);
            if !self.is_anything_active() && self.contains_mouse(id) && self.mouse_down() {
                is_being_dragged = true;
                state.drag_start = self.mouse.cursor_position;
                self.set_active(id);
                self.next_ids.0.dragged = id;
            }
        }

        let history = std::mem::take(&mut self.ui.rect_history);
        let padding = self.theme.padding as i32;
        self.ui.bounds = UiRect {
            min_x: state.pos.x,
            min_y: state.pos.y,
            max_x: state.pos.x + state.size.x + padding,
            max_y: state.pos.y + state.size.y + padding,
        };
        let last_scissor = self.ui.scissor_idx;
        if is_being_dragged {
            // Ensure that the widget is rendered on screen by pushing a new scissor that holds the
            // widget.
            // Only do this for the dragged widget, otherwise a lot of redundant scissors are
            // created.
            self.push_scissor(self.ui.bounds);
        }
        let layer = self.ui.layer;
        self.ui.layer += 1;
        if is_being_dragged {
            self.ui.layer += 100;
        }
        self.ui.id_stack.push(0);
        ///////////////////////
        contents(self);
        ///////////////////////
        self.ui.layer = layer;
        self.ui.id_stack.pop();
        self.ui.bounds = old_bounds;
        let child_history = std::mem::replace(&mut self.ui.rect_history, history);
        let mut content_bounds = bounding_rect(&child_history);

        if is_being_dragged {
            self.color_rect_from_rect(content_bounds, self.theme.primary_color, layer);
            self.ui.rect_history.pop();
            content_bounds.move_to_x(state.drag_anchor.x + state.size.x / 2);
            content_bounds.move_to_y(state.drag_anchor.y + state.size.y / 2);
            self.ui.rect_history.push(content_bounds);
        } else {
            self.ui.rect_history.extend_from_slice(&child_history);
            state.drag_anchor = IVec2::new(content_bounds.min_x, content_bounds.min_y);
            state.pos = state.drag_anchor;
        }
        self.ui.scissor_idx = last_scissor;

        self.submit_rect(id, content_bounds);
        state.size = IVec2::new(content_bounds.width(), content_bounds.height());

        self.insert_memory(id, state);

        DragResponse {
            is_being_dragged,
            inner: Response {
                hovered: self.is_hovered(id),
                active: is_being_dragged,
                rect: content_bounds,
                inner: (),
            },
        }
    }

    /// return the previous layer
    fn push_layer(&mut self) -> u16 {
        let l = self.ui.layer;
        self.ui.layer += 1;
        l
    }

    pub fn drop_target(&mut self, mut contents: impl FnMut(&mut Self, DropState)) -> DropResponse {
        self.begin_widget();
        let id = self.current_id();
        let old_bounds = self.ui.bounds;
        let layer = self.push_layer();
        self.ui.id_stack.push(0);
        let mut state = DropState::default();
        state.id = id;
        state.dragged = self.ids.dragged;
        if self.is_anything_dragged() {
            if self.contains_mouse(id) {
                state.hovered = true;
                if self.mouse_up() {
                    state.dropped = true;
                }
            }
        }
        let history_start = self.ui.rect_history.len();
        ///////////////////////
        contents(self, state);
        ///////////////////////
        self.ui.layer = layer;
        self.ui.id_stack.pop();
        self.ui.bounds = old_bounds;

        let content_bounds = self.history_bounding_rect(history_start);
        self.submit_rect(id, content_bounds);

        if state.hovered {
            let color = self.theme.primary_color;
            self.color_rect_from_rect(content_bounds, color, layer);
            self.ui.rect_history.pop();
        }

        DropResponse {
            dropped: state.dropped,
            inner: Response {
                hovered: self.is_hovered(id),
                active: state.dropped,
                rect: content_bounds,
                inner: (),
            },
        }
    }

    pub fn text_input(&mut self, content: &mut String) -> Response<()> {
        self.begin_widget();
        let id = self.current_id();
        let last_layer = self.push_layer();
        let layer = self.ui.layer;

        let mut state = self
            .get_memory_or_insert::<TextInputState>(id, || TextInputState {
                cursor: content.len(),
                ..Default::default()
            })
            .clone();

        state.cursor = state.cursor.min(content.len());
        let mouse_pos = self.mouse.cursor_position;

        // handle input
        let is_active = self.is_active(id);
        if is_active {
            state.caret_timer.update(self.delta_time.0);
            state.cursor_debounce.update(self.delta_time.0);
            if state.cursor_debounce.just_finished() {
                state.can_move = true;
            }
            if state.caret_timer.just_finished() {
                state.show_caret = !state.show_caret;
            }
            macro_rules! cursor_update {
                ($inner: tt) => {
                    if state.can_move {
                        $inner
                        state.can_move = false;
                        state.cursor_debounce.reset();
                    }
                };
            }

            for k in self.keyboard.pressed.iter() {
                match k {
                    KeyCode::ArrowLeft => {
                        cursor_update!({
                            state.cursor = state.cursor.saturating_sub(1);
                        });
                    }
                    KeyCode::ArrowRight => {
                        cursor_update!({
                            state.cursor = content.len().min(state.cursor + 1);
                        });
                    }
                    KeyCode::Home => {
                        cursor_update!({
                            state.cursor = 0;
                        });
                    }
                    KeyCode::End => {
                        cursor_update!({
                            state.cursor = content.len();
                        });
                    }
                    KeyCode::Backspace => {
                        cursor_update!({
                            if state.cursor > 0 {
                                state.cursor -= 1;
                                content.remove(state.cursor);
                            }
                        });
                    }
                    KeyCode::Delete => {
                        cursor_update!({
                            if state.cursor < content.len() {
                                content.remove(state.cursor);
                            }
                        });
                    }
                    _ => {
                        if let Some(text) = self
                            .keyboard
                            .events
                            .get(k)
                            .and_then(|ev| ev.logical_key.to_text())
                        {
                            content.insert_str(state.cursor, text);
                            state.cursor += text.len();
                        }
                    }
                }
            }
        } else if self.is_hovered(id) {
            if !self.contains_mouse(id) {
                self.set_not_hovered(id);
            } else if self.mouse_down() {
                self.set_not_hovered(id);
                self.set_active(id);
            }
        }
        if self.contains_mouse(id) {
            self.set_hovered(id);
        }

        // shape the text
        let mut w = 0;
        let mut h = 0;
        let x = self.ui.bounds.min_x;
        let y = self.ui.bounds.min_y;
        let padding = self.theme.padding as i32;
        let [x, y] = [x + padding, y + padding];
        if !content.is_empty() {
            let mouse_up = self.mouse_up();
            let (handle, e) =
                self.shape_and_draw_line(content.clone(), self.theme.font_size as u32);
            let pic = &e.texture;
            let line_width = pic.width() as i32;
            let line_height = pic.height() as i32;
            w = w.max(line_width);
            h += line_height;

            if is_active && mouse_up {
                // remap mouse position into 'shape space'
                let mx = mouse_pos.x - x as f64;
                let my = mouse_pos.y - y as f64;
                let width = e.texture.bounds.bounds.width() as f64;
                let height = e.texture.bounds.bounds.width() as f64;

                let sx = width / line_width as f64;
                let sy = height / line_height as f64;

                let mx = mx * sx;
                let my = my * sy;

                for (cluster, glyph_bounds) in e.texture.bounds.glyph_bounds.iter() {
                    // find the cluster where the pointer points to
                    if glyph_bounds.contains_point(mx as i32, my as i32) {
                        state.cursor = *cluster as usize;
                        debug!("Setting cursor to {}", state.cursor);
                        break;
                    }
                }
            }

            self.text_rect(
                x,
                y,
                line_width,
                line_height,
                self.theme.primary_color,
                layer + 1,
                handle,
            );

            if is_active && state.show_caret {
                // caret

                // TODO: better position
                let t = state.cursor as f64 / content.len() as f64;
                let cx = line_width as f64 * t;
                self.color_rect(
                    x + cx as i32,
                    y,
                    1,
                    line_height,
                    self.theme.primary_color,
                    layer + 2,
                );
            }
        }

        if is_active && self.mouse_up() && !self.contains_mouse(id) {
            self.set_not_active(id);
        }

        let w = w.max(self.theme.font_size as i32 * 10);
        let h = h.max(self.theme.font_size as i32);
        let rect = UiRect {
            min_x: x,
            min_y: y,
            max_x: x + w,
            max_y: y + h,
        };
        self.color_rect_from_rect(rect, self.theme.secondary_color, layer);
        self.submit_rect(id, rect);
        self.ui.layer = last_layer;
        self.insert_memory(id, state);
        Response {
            hovered: self.ids.hovered == id,
            active: self.ids.active == id,
            inner: (),
            rect,
        }
    }
}

#[derive(Debug, Default)]
struct DragState {
    pub drag_start: PhysicalPosition<f64>,
    pub drag_anchor: IVec2,
    pub pos: IVec2,
    pub size: IVec2,
}

#[derive(Debug)]
pub struct DragResponse {
    pub is_being_dragged: bool,
    pub inner: Response<()>,
}

#[derive(Debug)]
pub struct DropResponse {
    pub dropped: bool,
    pub inner: Response<()>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DropState {
    pub id: UiId,
    pub dragged: UiId,
    pub dropped: bool,
    pub hovered: bool,
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

#[derive(Debug)]
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
        ctx.ui.bounds.min_x = self.dims[idx][0];
        ctx.ui.bounds.max_x = self.dims[idx][1];
        let w = ctx.ui.bounds.width();
        *ctx.ui.id_stack.last_mut().unwrap() = i;
        let layer = ctx.ui.layer;
        ctx.ui.layer += 1;
        ctx.ui.id_stack.push(0);
        let history_start = ctx.ui.rect_history.len();

        ///////////////////////
        contents(ctx);
        ///////////////////////

        // restore state
        ctx.ui.id_stack.pop();
        let rect = ctx.history_bounding_rect(history_start);
        ctx.ui.bounds.min_y = bounds.min_y;
        ctx.ui.bounds.max_y = bounds.max_y;
        if rect.width() > w && i + 1 < self.cols {
            let diff = rect.width() - w;
            for d in &mut self.dims[idx + 1..] {
                d[0] += diff;
                d[1] += diff;
            }
        }
        ctx.ui.layer = layer;
    }
}

fn begin_frame(mut ui: ResMut<UiState>, window_size: Res<crate::renderer::WindowSize>) {
    ui.layout_dir = LayoutDirection::TopDown;
    ui.root_hash = 0;
    ui.rect_history.clear();
    ui.color_rects.clear();
    ui.text_rects.clear();
    ui.bounds = UiRect {
        min_x: 0,
        min_y: 0,
        max_x: window_size.width as i32,
        max_y: window_size.height as i32,
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
    let mut rects_consumed = 0;
    for (g, (rects, sc, _id)) in
        (color_rects.chunk_by(|a, b| a.scissor == b.scissor)).zip(color_rect_q.iter_mut())
    {
        buffers_reused += 1;
        rects_consumed += g.len();
        rects.0.clear();
        rects.0.extend_from_slice(g);
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
    }
    for (_, _, id) in color_rect_q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in color_rects[rects_consumed..].chunk_by(|a, b| a.scissor == b.scissor) {
        cmd.spawn().insert_bundle((
            RectRequests(g.to_vec()),
            UiScissor(ui.scissors[g[0].scissor as usize]),
        ));
    }
    ui.color_rects = color_rects;
    ui.color_rects.clear();
}

// preserve the buffers by zipping together a query with the chunks, spawn new if not enough,
// GC if too many
// most frames should have the same items
fn submit_frame_text_rects(
    mut ui: ResMut<UiState>,
    mut cmd: Commands,
    mut text_rect_q: Query<(&mut TextRectRequests, &mut UiScissor, EntityId)>,
) {
    let mut text_rects = std::mem::take(&mut ui.text_rects);
    text_rects.sort_unstable_by_key(|r| r.scissor);

    let mut buffers_reused = 0;
    let mut rects_consumed = 0;
    for (g, (rects, sc, _id)) in
        (text_rects.chunk_by_mut(|a, b| a.scissor == b.scissor)).zip(text_rect_q.iter_mut())
    {
        buffers_reused += 1;
        rects_consumed += g.len();
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
        rects.0.clear();
        rects.0.extend(g.iter_mut().map(|x| std::mem::take(x)));
    }
    for (_, _, id) in text_rect_q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in text_rects[rects_consumed..].chunk_by_mut(|a, b| a.scissor == b.scissor) {
        cmd.spawn().insert_bundle((
            UiScissor(ui.scissors[g[0].scissor as usize]),
            TextRectRequests(g.iter_mut().map(|x| std::mem::take(x)).collect()),
        ));
    }
    ui.text_rects = text_rects;
    ui.text_rects.clear();
}

pub struct Ui<'a> {
    ids: Res<'a, UiIds>,
    next_ids: ResMut<'a, NextUiIds>,
    ui: ResMut<'a, UiState>,
    texture_cache: ResMut<'a, TextTextureCache>,
    shaping_results: ResMut<'a, assets::Assets<ShapingResult>>,
    theme: ResMut<'a, Theme>,
    mouse: Res<'a, MouseInputs>,
    keyboard: Res<'a, KeyBoardInputs>,
    memory: ResMut<'a, UiMemory>,
    fonts: Res<'a, Assets<OwnedTypeFace>>,
    delta_time: Res<'a, DeltaTime>,
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

#[derive(Default, Debug)]
struct WindowAllocator {
    pub next: IVec2,
}

impl WindowAllocator {
    pub fn next_pos(&mut self, size: IVec2, bounds: UiRect) -> IVec2 {
        let res = self.next;
        if self.next.x + size.x < bounds.max_x {
            self.next.x += size.x;
        } else {
            self.next.y += size.y;
            self.next.x = bounds.min_x;
            if self.next.y > bounds.max_y {
                self.next.y = bounds.min_y;
            }
        }
        res
    }
}

impl<'a> UiRoot<'a> {
    pub fn window(&mut self, desc: WindowDescriptor, mut contents: impl FnMut(&mut Ui)) {
        let mut allocator = std::mem::take(&mut self.0.ui.window_allocator);
        let old_bounds = self.0.ui.bounds;
        let state: &mut WindowState = self
            .0
            .ui
            .windows
            .entry(desc.name.to_owned())
            .or_insert_with(|| {
                // TODO: allocate window
                let initial_size = IVec2::splat(200);
                let pos = allocator.next_pos(initial_size, old_bounds);
                WindowState {
                    pos: desc.pos.unwrap_or(pos),
                    size: desc.size.unwrap_or(initial_size),
                    drag_anchor: Default::default(),
                    drag_start: Default::default(),
                    content_size: IVec2::ZERO,
                }
            });

        let padding = self.0.theme.window_padding as i32;
        let width = state.size.x;
        let height = state.size.y - self.0.theme.window_title_height as i32;
        let bounds = UiRect {
            min_x: state.pos.x,
            min_y: state.pos.y + self.0.theme.window_title_height as i32,
            max_x: state.pos.x + width,
            max_y: state.pos.y + self.0.theme.window_title_height as i32 + height,
        };
        let title_bounds = UiRect {
            min_x: state.pos.x,
            min_y: state.pos.y,
            max_x: state.pos.x + width + 2 * padding,
            max_y: state.pos.y + self.0.theme.window_title_height as i32,
        };

        self.0.ui.root_hash = fnv_1a(desc.name.as_bytes());
        let scissor = self.0.ui.scissor_idx;

        let layer = self.0.ui.layer;
        self.0.ui.layer = WINDOW_LAYER;
        // window background
        self.0.color_rect(
            bounds.min_x,
            bounds.min_y,
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
                if !self.0.is_anything_active()
                    && self.0.contains_mouse(title_id)
                    && self.0.mouse_down()
                {
                    let state: &mut WindowState = self.0.ui.windows.get_mut(desc.name).unwrap();
                    state.drag_start = self.0.mouse.cursor_position;
                    state.drag_anchor = state.pos;
                    self.0.set_active(title_id);
                }
            }
            self.0.submit_rect(title_id, title_bounds);
            self.0
                .color_rect_from_rect(title_bounds, 0x00ffffff, WINDOW_LAYER);
        }
        ///////////////////////
        ///////////////////////
        // Content
        let history_start = self.0.ui.rect_history.len();
        {
            self.0.push_scissor(bounds);
            let mut bounds = bounds;
            bounds.shrink_x(padding);
            bounds.shrink_y(padding);
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
        state.content_size = IVec2::new(r.width(), r.height());
        state.size = state.content_size + 2 * IVec2::splat(padding);
        state.size.y = (state.size.y).max(5) + self.0.theme.window_title_height as i32;
        self.0.ui.window_allocator = allocator;
    }

    pub fn panel(&mut self, desc: PanelDescriptor, mut contents: impl FnMut(&mut Ui)) {
        let width = desc.width.as_abolute(self.0.ui.bounds.width());
        let height = desc.height.as_abolute(self.0.ui.bounds.height());

        let old_bounds = self.0.ui.bounds;
        let mut bounds = old_bounds;
        bounds.resize_w(width);
        bounds.resize_h(height);

        match desc.horizonal {
            HorizontalAlignment::Left => {
                let delta = -bounds.min_x;
                bounds.offset_x(delta);
            }
            HorizontalAlignment::Right => {
                let delta = old_bounds.max_x - bounds.max_x;
                bounds.offset_x(delta);
            }
            HorizontalAlignment::Center => {}
        }
        match desc.vertical {
            VerticalAlignment::Top => {
                let delta = -bounds.min_y;
                bounds.offset_y(delta);
            }
            VerticalAlignment::Bottom => {
                let delta = old_bounds.max_y - bounds.max_y;
                bounds.offset_y(delta);
            }
            VerticalAlignment::Center => {}
        }
        self.0.ui.root_hash = fnv_1a(bytemuck::cast_slice(&[
            bounds.min_x,
            bounds.min_y,
            width,
            height,
        ]));
        self.0.ui.bounds = bounds;
        let scissor = self.0.push_scissor(bounds);

        let layer = self.0.ui.layer;
        self.0.ui.layer += 1;
        self.0.color_rect(
            bounds.min_x,
            bounds.min_y,
            width,
            height,
            0x04a5e5ff,
            self.0.ui.layer,
        );
        self.0.ui.layer += 1;
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
        let ids = Res::new(db);
        let next_ids = ResMut::new(db);
        let delta_time = Res::new(db);
        Self(Ui {
            ids,
            next_ids,
            ui,
            texture_cache,
            shaping_results: text_assets,
            theme,
            mouse,
            keyboard,
            memory,
            fonts,
            delta_time,
        })
    }

    fn resources_mut(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<NextUiIds>());
        set.insert(TypeId::of::<UiState>());
        set.insert(TypeId::of::<TextTextureCache>());
        set.insert(TypeId::of::<Assets<ShapingResult>>());
        set.insert(TypeId::of::<Theme>());
        set.insert(TypeId::of::<UiMemory>());
    }

    fn resources_const(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<DeltaTime>());
        set.insert(TypeId::of::<MouseInputs>());
        set.insert(TypeId::of::<UiIds>());
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

    let UiRect {
        mut min_x,
        mut min_y,
        mut max_x,
        mut max_y,
    } = history[0];

    for r in &history[1..] {
        min_x = min_x.min(r.min_x);
        max_x = max_x.max(r.max_x);

        min_y = min_y.min(r.min_y);
        max_y = max_y.max(r.max_y);
    }

    UiRect {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

#[inline]
fn div_half_ceil(n: i32) -> i32 {
    let d = n / 2;
    let r = n % 2;
    d + r
}

#[derive(Debug, Clone)]
struct TextInputState {
    cursor: usize,
    caret_timer: Timer,
    show_caret: bool,
    cursor_debounce: Timer,
    can_move: bool,
}

impl Default for TextInputState {
    fn default() -> Self {
        Self {
            cursor: Default::default(),
            caret_timer: Timer::new(Duration::from_millis(500), true),
            show_caret: false,
            cursor_debounce: Timer::new(Duration::from_millis(100), true),
            can_move: true,
        }
    }
}
