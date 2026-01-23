use crate::{
    assets::{self, Assets, AssetsPlugin, Handle, WeakHandle},
    DeltaTime, KeyBoardInputs, MouseInputs, Plugin, Tick, Timer,
};

pub mod color_rect_pipeline;
pub mod rect;
pub mod text;
pub mod text_rect_pipeline;
pub mod textured_rect_pipeline;

#[cfg(test)]
mod tests;

use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    hash::Hash,
    i32,
    ptr::NonNull,
    str::FromStr,
    time::Duration,
};

use cecs::{prelude::*, query};
use glam::IVec2;
use image::DynamicImage;
use text_rect_pipeline::{DrawTextRect, TextRectRequests};
use textured_rect_pipeline::{DrawTextureRect, TextureRectRequests};
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

#[derive(Clone, Copy, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct Color(pub u32);

impl std::fmt::Debug for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let rgba: [u8; 4] = bytemuck::cast(self.0);
        f.debug_struct("Color")
            .field("r", &rgba[0])
            .field("g", &rgba[1])
            .field("b", &rgba[2])
            .field("a", &rgba[3])
            .finish()
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::from_rgb(0)
    }
}

impl From<u32> for Color {
    fn from(value: u32) -> Self {
        Self::from_rgba(value)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ColorParseError {
    #[error("Color strings must begin with a hashmark (#)")]
    MissingHash,
    #[error("Invalid length of {0}. Expected length 4 (#rgb) or 7 (#rrggbb) or 5 (#rgba) or 9 (#rrggbbaa)")]
    BadLength(usize),
    #[error("Invalid character in the color string: {0} Expected hexadecimal number")]
    InvalidCharacter(char),
    #[error("Expected an ascii string")]
    NotAscii,
}

fn collapse_color(buffer: &mut [u8], l: usize) -> Color {
    for i in (0..(2 * l)).step_by(2) {
        buffer[i] = (buffer[i] * 16) + (buffer[i + 1]);
    }
    for i in 1..l {
        buffer[i] = buffer[i * 2];
    }
    Color::from_slice(&buffer[..l])
}

impl FromStr for Color {
    type Err = ColorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with('#') {
            return Err(ColorParseError::MissingHash);
        }

        // TODO: assert little endian
        let mut buffer = [0u8; 8];
        let mut l = 0;
        for c in s.chars().skip(1).take(8).map(|x| x.to_ascii_lowercase()) {
            let x = c as u8;
            let c = if b'0' <= x && x <= b'9' {
                x - b'0'
            } else if b'a' <= x && x <= b'f' {
                x - b'a' + 10
            } else {
                return Err(ColorParseError::InvalidCharacter(c));
            };
            buffer[l] = c;
            l += 1;
        }
        match l {
            3 => {
                for i in (0..6).step_by(2).rev() {
                    buffer[i] = buffer[i / 2];
                    buffer[i + 1] = buffer[i / 2];
                }
                Ok(collapse_color(&mut buffer, 3))
            }
            4 => {
                for i in (0..8).step_by(2).rev() {
                    buffer[i] = buffer[i / 2];
                    buffer[i + 1] = buffer[i / 2];
                }
                Ok(collapse_color(&mut buffer, 4))
            }
            6 => Ok(collapse_color(&mut buffer, 3)),
            8 => Ok(collapse_color(&mut buffer, 4)),
            _ => Err(ColorParseError::BadLength(l + 1)),
        }
    }
}

impl Color {
    pub const BLACK: Self = Color::from_rgb(0);
    pub const RED: Self = Color::from_rgb(0xFF0000);
    pub const YELLOW: Self = Color::from_rgb(0xFFFF00);
    pub const GREEN: Self = Color::from_rgb(0x00FF00);
    pub const BLUE: Self = Color::from_rgb(0x0000FF);
    pub const WHITE: Self = Color::from_rgb(0xFFFFFF);
    pub const TRANSPARENT: Self = Color::from_rgba(0);

    pub const fn from_rgba(rgba: u32) -> Self {
        Self(rgba)
    }

    pub const fn from_rgb(rgb: u32) -> Self {
        Self((rgb << 8) | 0xFF)
    }

    /// consumes at most 4 items, in order rgba
    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut res = 0;

        for (i, b) in bytes.iter().enumerate().take(4) {
            res |= (*b as u32) << (24 - 8 * i);
        }

        if bytes.len() < 4 {
            res |= 0xFF;
        }

        // only 1 item has been submitted, splat it
        if res & 0xFF0000FF == res {
            let byte = res >> 24;
            res |= byte << 16;
            res |= byte << 8;
        }

        Color(res)
    }

    pub const fn splat_rgb(byte: u8) -> Self {
        let byte = byte as u32;
        Self(byte << 24 | byte << 16 | byte << 8 | 0xFF)
    }

    pub fn as_rgba(&self) -> [u8; 4] {
        bytemuck::cast(self.0)
    }

    pub fn as_rgb(&self) -> [u8; 3] {
        let a = self.as_rgba();
        [a[0], a[1], a[2]]
    }
}

#[derive(Debug, Default)]
pub struct UiDebug {
    pub enable: bool,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(self, app: &mut crate::App) {
        app.add_plugin(color_rect_pipeline::UiColorRectPlugin);
        app.add_plugin(text_rect_pipeline::UiTextRectPlugin);
        app.add_plugin(textured_rect_pipeline::UiTextureRectPlugin);
        app.require_plugin(AssetsPlugin::<OwnedTypeFace>::default());
        app.require_plugin(AssetsPlugin::<ShapingResult>::default());
        app.require_plugin(AssetsPlugin::<DynamicImage>::default());

        app.insert_resource(UiState::new());
        app.insert_resource(UiIds::default());
        app.insert_resource(NextUiIds(Default::default()));
        app.insert_resource(TextTextureCache::default());
        app.insert_resource(UiMemory::default());
        app.insert_resource(UiInputs::default());

        if app.get_resource::<Theme>().is_none() {
            app.insert_resource(Theme::default());
        }

        app.with_stage(crate::Stage::PreUpdate, |s| {
            s.add_system(begin_frame);
        });
        app.with_nested_stage(
            crate::Stage::Update,
            SystemStage::new("debug")
                .with_should_run(|debug: Option<Res<UiDebug>>| {
                    debug.map(|d| d.enable).unwrap_or(false)
                })
                .with_system(draw_bounding_boxes),
        );
        app.with_stage(crate::Stage::PostUpdate, |s| {
            s.add_system(submit_frame_color_rects)
                .add_system(submit_frame_text_rects)
                .add_system(submit_frame_texture_rects)
                .add_system(update_ids)
                .add_system(shaping_gc_system);
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
    last_access: u64,
}

/// assign new ids, lhs = rhs
fn update_ids(mut lhs: ResMut<UiIds>, mut rhs: ResMut<NextUiIds>) {
    let ids = &mut rhs.0;
    ids.sort_by_key(|x| x.layer);
    for mut idset in ids.drain(..) {
        if idset.has_added_flag(InteractionFlag::Hovered) {
            lhs.hovered.insert(idset.id);
            lhs.top_hovered = idset.id;
        }
        if idset.has_added_flag(InteractionFlag::Active) {
            lhs.active = idset.id;
        }
        if idset.has_added_flag(InteractionFlag::Scrolling) {
            lhs.scrolling = idset.id;
        }
        if idset.has_added_flag(InteractionFlag::Dragged) {
            lhs.active = idset.id;
            lhs.dragged = idset.id;
        }
        if idset.has_added_flag(InteractionFlag::ContextMenu) {
            lhs.context_menu = idset.id;
        }
        if lhs.dragged == idset.id && idset.has_removed_flag(InteractionFlag::Dragged) {
            lhs.dragged = UiId::SENTINEL;
            if !idset.has_added_flag(InteractionFlag::Active) {
                idset.remove_flag(InteractionFlag::Active);
            }
        }
        if idset.has_removed_flag(InteractionFlag::Hovered) {
            lhs.hovered.remove(&idset.id);
            if lhs.top_hovered == idset.id {
                lhs.top_hovered = UiId::SENTINEL;
            }
        }
        if lhs.active == idset.id && idset.has_removed_flag(InteractionFlag::Active) {
            lhs.active = UiId::SENTINEL;
        }
        if lhs.scrolling == idset.id && idset.has_removed_flag(InteractionFlag::Scrolling) {
            lhs.scrolling = UiId::SENTINEL;
        }
        if idset.has_removed_flag(InteractionFlag::ContextMenu) {
            lhs.context_menu = UiId::SENTINEL;
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct UiIds {
    hovered: HashSet<UiId>,
    /// Topmost hovered widget
    top_hovered: UiId,
    active: UiId,
    dragged: UiId,
    context_menu: UiId,
    scrolling: UiId,
}
pub struct NextUiIds(pub Vec<NextUiIdSet>);

impl NextUiIds {
    pub fn push(&mut self, id: UiId, layer: u16) -> &mut NextUiIdSet {
        if self.0.last_mut().map(|x| x.id != id).unwrap_or(true) {
            self.0.push(NextUiIdSet {
                layer,
                id,
                added_flags: 0,
                removed_flags: 0,
            });
        }
        self.0.last_mut().unwrap()
    }
}

// bit flags
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InteractionFlag {
    Hovered = 1,
    Active = 2,
    Dragged = 4,
    ContextMenu = 8,
    Scrolling = 16,
}

impl InteractionFlag {
    fn write_interaction_flags(f: &mut std::fmt::Formatter<'_>, flags: u8) -> std::fmt::Result {
        write!(f, "[ ")?;
        let mut has = false;
        if (flags & (Self::Hovered as u8)) != 0 {
            write!(f, "Hovered")?;
            has = true;
        }
        if (flags & (Self::Active as u8)) != 0 {
            if has {
                write!(f, " ")?;
            }
            write!(f, "Active")?;
            has = true;
        }
        if (flags & (Self::Dragged as u8)) != 0 {
            if has {
                write!(f, " ")?;
            }
            write!(f, "Dragged")?;
            has = true;
        }
        if (flags & (Self::ContextMenu as u8)) != 0 {
            if has {
                write!(f, " ")?;
            }
            write!(f, "ContextMenu")?;
        }
        if (flags & (Self::Scrolling as u8)) != 0 {
            if has {
                write!(f, " ")?;
            }
            write!(f, "Scrolling")?;
        }
        write!(f, " ]")
    }
}

#[derive(Default, Clone, Copy)]
pub struct NextUiIdSet {
    pub id: UiId,
    pub layer: u16,
    pub added_flags: u8,
    pub removed_flags: u8,
}

impl std::fmt::Debug for NextUiIdSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NextUiIdSet")
            .field("id", &self.id)
            .field("layer", &self.layer)
            .field_with("added_flags", |f| {
                InteractionFlag::write_interaction_flags(f, self.added_flags)
            })
            .field_with("removed_flags", |f| {
                InteractionFlag::write_interaction_flags(f, self.removed_flags)
            })
            .finish()
    }
}

impl NextUiIdSet {
    pub fn add_flag(&mut self, flag: InteractionFlag) {
        self.added_flags |= flag as u8;
        self.removed_flags &= !(flag as u8);
    }

    pub fn remove_flag(&mut self, flag: InteractionFlag) {
        self.added_flags &= !(flag as u8);
        self.removed_flags |= flag as u8;
    }

    pub fn has_added_flag(&self, flag: InteractionFlag) -> bool {
        (self.added_flags & flag as u8) != 0
    }

    pub fn has_removed_flag(&self, flag: InteractionFlag) -> bool {
        (self.removed_flags & flag as u8) != 0
    }
}

/// UI context object. Use this to builder your user interface
pub struct UiState {
    /// Stack of parents in the UI tree
    id_stack: Vec<IdxType>,
    widget_ids: Vec<UiId>,

    color_rects: Vec<DrawColorRect>,
    texture_rects: Vec<DrawTextureRect>,
    text_rects: Vec<DrawTextRect>,
    scissors: Vec<UiRect>,
    scissor_idx: u32,
    bounds: UiRect,

    /// Layers go from back to front
    layer: u16,

    bounding_boxes: HashMap<UiId, UiRect>,

    rect_history: Vec<UiRect>,

    /// Hash of the current tree root
    root_hash: u32,

    layout_dir: LayoutDirection,

    /// TODO: gc?
    windows: HashMap<String, WindowState>,
    fallback_font: OwnedTypeFace,

    window_allocator: WindowAllocator,
}

#[derive(Debug, Clone, Default)]
pub struct UiInputs {
    pub wants_keyboard: bool,
    /// keys consumed by the UI
    pub keys: HashSet<KeyCode>,
}

impl UiInputs {
    pub fn clear(&mut self) {
        self.wants_keyboard = false;
        self.keys.clear();
    }

    pub fn wants_input(&self) -> bool {
        self.wants_keyboard() || self.wants_mouse()
    }

    pub fn wants_keyboard(&self) -> bool {
        self.wants_keyboard || !self.keys.is_empty()
    }

    pub fn wants_mouse(&self) -> bool {
        // TODO:
        false
    }
}

#[derive(Clone)]
pub enum ThemeEntry {
    Color(Color),
    Image(Handle<DynamicImage>),
}

impl From<Color> for ThemeEntry {
    fn from(value: Color) -> Self {
        Self::Color(value)
    }
}

impl From<u32> for ThemeEntry {
    fn from(value: u32) -> Self {
        Self::Color(Color::from_rgba(value))
    }
}

impl From<Handle<DynamicImage>> for ThemeEntry {
    fn from(value: Handle<DynamicImage>) -> Self {
        Self::Image(value)
    }
}

#[derive(Clone)]
pub struct Theme {
    pub background: ThemeEntry,
    pub window_background: ThemeEntry,
    pub primary_color: Color,
    pub secondary_color: Color,
    pub button_default: ThemeEntry,
    pub button_hovered: Option<ThemeEntry>,
    pub button_pressed: Option<ThemeEntry>,
    /// if unavailable, then fall back to primary_color
    pub button_text_color: Option<Color>,

    pub context_background: ThemeEntry,

    pub drop_target_default: ThemeEntry,
    pub drop_target_hovered: Option<ThemeEntry>,

    pub text_padding: u16,
    pub font_size: u16,
    pub scroll_bar_size: u16,
    pub window_title_height: u8,
    pub font: Handle<OwnedTypeFace>,
    pub window_padding: u8,

    pub padding: Padding,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            window_background: 0x0395d5ff.into(),
            background: 0x04a5e5ff.into(),
            context_background: 0x02a3e3ff.into(),
            primary_color: 0xcdd6f4ff.into(),
            secondary_color: 0x212224ff.into(),
            button_default: 0x212224ff.into(),
            button_hovered: Some(0x45475aff.into()),
            button_pressed: Some(0x585b70ff.into()),
            button_text_color: None,

            drop_target_default: 0x0.into(),
            drop_target_hovered: Some(0xcdd6f4ff.into()),

            text_padding: 5,
            font_size: 12,
            padding: Padding::splat(5),
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
            widget_ids: Default::default(),
            color_rects: Default::default(),
            text_rects: Default::default(),
            texture_rects: Default::default(),
            scissors: Default::default(),
            scissor_idx: 0,
            bounds: Default::default(),
            layer: 0,
            bounding_boxes: Default::default(),
            rect_history: Default::default(),
            root_hash: 0,
            layout_dir: LayoutDirection::TopDown(HorizontalAlignment::Left),
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

#[derive(Default, Clone)]
pub struct ThemeOverride {
    pub background: Option<ThemeEntry>,
    pub window_background: Option<ThemeEntry>,
    pub primary_color: Option<Color>,
    pub secondary_color: Option<Color>,
    pub button_default: Option<ThemeEntry>,
    pub button_hovered: Option<Option<ThemeEntry>>,
    pub button_pressed: Option<Option<ThemeEntry>>,
    pub button_text_color: Option<Option<Color>>,

    pub context_background: Option<ThemeEntry>,

    pub drop_target_default: Option<ThemeEntry>,
    pub drop_target_hovered: Option<Option<ThemeEntry>>,

    pub text_padding: Option<u16>,
    pub font_size: Option<u16>,
    pub padding: Option<Padding>,
    pub scroll_bar_size: Option<u16>,
    pub window_title_height: Option<u8>,
    pub font: Option<Handle<OwnedTypeFace>>,
    pub window_padding: Option<u8>,
}

impl ThemeOverride {
    /// The returned ThemeOverride will revert the effects of this apply
    pub fn apply(mut self, theme: &mut Theme) -> Self {
        macro_rules! apply {
            ($field: ident) => {
                if let Some(x) = self.$field.take() {
                    self.$field = Some(std::mem::replace(&mut theme.$field, x));
                }
            };
        }
        apply!(background);
        apply!(window_background);
        apply!(primary_color);
        apply!(secondary_color);
        apply!(button_default);
        apply!(button_hovered);
        apply!(button_pressed);
        apply!(button_text_color);
        apply!(context_background);
        apply!(drop_target_default);
        apply!(drop_target_hovered);
        apply!(text_padding);
        apply!(font_size);
        apply!(padding);
        apply!(scroll_bar_size);
        apply!(window_title_height);
        apply!(font);
        apply!(window_padding);

        self
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LayoutDirection {
    Center,
    TopDown(HorizontalAlignment),
    BottomUp(HorizontalAlignment),
    LeftRight(VerticalAlignment),
    RightLeft(VerticalAlignment),
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
                (max as f64 * p).round() as i32
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

fn shaping_gc_system(
    mut texture_cache: ResMut<TextTextureCache>,
    shaping_results: Res<assets::Assets<ShapingResult>>,
    tick: Res<Tick>,
) {
    // maximum number of items to be collected per tick
    // TODO: configure
    let mut max = 100;
    texture_cache.0.retain(|_, k| {
        if max == 0 {
            return true;
        }
        let Some(sh) = shaping_results.get_by_id(k.id()) else {
            max -= 1;
            return false;
        };
        // TODO: configure ticks
        let retain = tick.0.saturating_sub(sh.last_access) <= 120;
        if !retain {
            max -= 1;
        }
        retain
    });
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
        self.next_ids
            .push(id, self.ui.layer)
            .add_flag(InteractionFlag::Hovered);
    }

    #[inline]
    fn set_context_menu(&mut self, id: UiId) {
        self.next_ids
            .push(id, self.ui.layer)
            .add_flag(InteractionFlag::ContextMenu);
    }

    #[inline]
    fn set_scrolling(&mut self, id: UiId) {
        self.next_ids
            .push(id, self.ui.layer)
            .add_flag(InteractionFlag::Scrolling);
    }

    #[inline]
    fn set_active(&mut self, id: UiId) {
        self.next_ids
            .push(id, self.ui.layer)
            .add_flag(InteractionFlag::Active);
    }

    #[inline]
    pub fn is_anything_dragged(&self) -> bool {
        self.ids.dragged != UiId::SENTINEL
    }

    #[inline]
    pub fn is_scrolling(&self, id: UiId) -> bool {
        self.ids.scrolling == id
    }

    #[inline]
    pub fn is_anything_scrolling(&self) -> bool {
        self.ids.scrolling != UiId::SENTINEL
    }

    #[inline]
    pub fn is_context_menu_open(&self) -> bool {
        self.ids.context_menu != UiId::SENTINEL
    }

    pub fn clear_active(&mut self) {
        self.next_ids
            .push(UiId::SENTINEL, self.ui.layer)
            .remove_flag(InteractionFlag::Active);
    }

    pub fn current_id(&self) -> UiId {
        self.ui
            .id_stack
            .last()
            .copied()
            .and_then(|i| self.ui.widget_ids.get(i as usize))
            .copied()
            .unwrap_or_default()
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
        self.ids.hovered.contains(&id)
    }

    #[inline]
    pub fn is_top_hovered(&self, id: UiId) -> bool {
        self.ids.top_hovered == id
    }

    #[inline]
    pub fn has_context_menu(&self, id: UiId) -> bool {
        self.ids.context_menu == id
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
            self.next_ids
                .push(id, self.ui.layer)
                .remove_flag(InteractionFlag::Active);
        }
    }

    #[inline]
    fn set_not_scrolling(&mut self, id: UiId) {
        if self.ids.scrolling == id {
            self.next_ids
                .push(id, self.ui.layer)
                .remove_flag(InteractionFlag::Scrolling);
        }
    }

    #[inline]
    fn set_not_hovered(&mut self, id: UiId) {
        self.next_ids
            .push(id, self.ui.layer)
            .remove_flag(InteractionFlag::Hovered);
    }

    pub fn horizontal(
        &mut self,
        vertical_alignment: impl Into<Option<VerticalAlignment>>,
        contents: impl FnMut(&mut Self),
    ) {
        self._with_layout(
            contents,
            LayoutDirection::LeftRight(vertical_alignment.into().unwrap_or(VerticalAlignment::Top)),
        );
    }

    pub fn horizontal_rev(
        &mut self,
        vertical_alignment: impl Into<Option<VerticalAlignment>>,
        contents: impl FnMut(&mut Self),
    ) {
        self._with_layout(
            contents,
            LayoutDirection::RightLeft(vertical_alignment.into().unwrap_or(VerticalAlignment::Top)),
        );
    }

    pub fn vertical(
        &mut self,
        horizontal_alignment: impl Into<Option<HorizontalAlignment>>,
        contents: impl FnMut(&mut Self),
    ) {
        self._with_layout(
            contents,
            LayoutDirection::TopDown(
                horizontal_alignment
                    .into()
                    .unwrap_or(HorizontalAlignment::Left),
            ),
        );
    }

    pub fn vertical_rev(
        &mut self,
        horizontal_alignment: impl Into<Option<HorizontalAlignment>>,
        contents: impl FnMut(&mut Self),
    ) {
        self._with_layout(
            contents,
            LayoutDirection::BottomUp(
                horizontal_alignment
                    .into()
                    .unwrap_or(HorizontalAlignment::Left),
            ),
        );
    }

    fn _with_layout(&mut self, contents: impl FnMut(&mut Self), layout: LayoutDirection) {
        let id = self.begin_widget();
        let history_start = self.ui.rect_history.len();
        let bounds = self.ui.bounds;
        let layout = std::mem::replace(&mut self.ui.layout_dir, layout);
        ///////////////////////
        self.children_content(contents);
        ///////////////////////
        self.ui.layout_dir = layout;
        self.ui.bounds = bounds;
        self.submit_rect_group(id, history_start);
    }

    /// If `hide` is true, then the inner contents are not rendered
    /// Useful for keeping the Id stack consistent
    pub fn hidden(&mut self, hide: bool, mut contents: impl FnMut(&mut Self)) {
        let id = self.begin_widget();
        let history_start = self.ui.rect_history.len();
        if !hide {
            let bounds = self.ui.bounds;
            self.push_child();
            ///////////////////////
            contents(self);
            ///////////////////////
            self.pop_child();
            self.ui.bounds = bounds;
        }
        self.submit_rect_group(id, history_start);
    }

    /// submit a new rect that contains all rects submitted beginning at history_start index
    fn submit_rect_group(&mut self, id: UiId, history_start: usize) -> UiRect {
        if self.ui.rect_history.len() <= history_start {
            // no rects have been submitted
            return UiRect::default();
        }

        let mut rect = self.ui.rect_history[history_start];
        self.ui.rect_history[history_start + 1..]
            .iter()
            .for_each(|r| rect = rect.grow_over(*r));
        self.submit_rect(id, rect, self.theme.padding);
        rect
    }

    pub fn grid<'b>(&mut self, columns: u32, mut contents: impl FnMut(&mut Columns) + 'b)
    where
        'a: 'b,
    {
        self.begin_widget();
        self.push_child();
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

        self.pop_child();
        self.ui.bounds = bounds;
        self.submit_rect_group(self.current_id(), history_start);
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

    pub fn color_rect_from_rect_with_outline(
        &mut self,
        rect: UiRect,
        color: Color,
        layer: u16,
        outline_color: Color,
        outline_radius: u32,
    ) {
        self.color_rect_with_outline(
            rect.min_x,
            rect.min_y,
            rect.width(),
            rect.height(),
            color,
            outline_radius,
            outline_color,
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
        assert!(!self.ui.scissors.is_empty());
        let scissor = self.ui.scissor_idx;
        self.ui.color_rects.push(DrawColorRect {
            x,
            y,
            w: width,
            h: height,
            color: color.0,
            layer,
            scissor,
            ..Default::default()
        })
    }

    pub fn color_rect_with_outline(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: Color,
        outline_radius: u32,
        outline_color: Color,
        layer: u16,
    ) {
        assert!(!self.ui.scissors.is_empty());
        let scissor = self.ui.scissor_idx;
        self.ui.color_rects.push(DrawColorRect {
            x,
            y,
            w: width,
            h: height,
            color: color.0,
            layer,
            scissor,
            outline_color: outline_color.0,
            outline_radius,
        })
    }

    /// low level rendering method
    pub fn image_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        image: Handle<DynamicImage>,
        layer: u16,
    ) {
        assert!(!self.ui.scissors.is_empty());
        let scissor = self.ui.scissor_idx;
        self.ui.texture_rects.push(DrawTextureRect {
            x,
            y,
            w: width,
            h: height,
            image,
            layer,
            scissor,
        });
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
        assert!(!self.ui.scissors.is_empty());
        let scissor = self.ui.scissor_idx;
        self.ui.text_rects.push(DrawTextRect {
            x,
            y,
            w: width,
            h: height,
            color: color.0,
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
                    last_access: 0,
                };

                self.shaping_results.insert(shaping)
            });

        let shape = self.shaping_results.get_mut(handle);
        shape.last_access = self.tick.0;
        (handle.clone(), shape)
    }

    pub fn with_theme(&mut self, theme: Theme, mut contents: impl FnMut(&mut Self)) {
        let t = std::mem::replace(&mut *self.theme, theme);

        ///////////////////////
        contents(self);
        ///////////////////////

        *self.theme = t;
    }

    pub fn with_theme_override(
        &mut self,
        theme: ThemeOverride,
        mut contents: impl FnMut(&mut Self),
    ) {
        let theme = theme.apply(&mut self.theme);

        ///////////////////////
        contents(self);
        ///////////////////////

        theme.apply(&mut self.theme);
    }

    pub fn image(
        &mut self,
        image: Handle<DynamicImage>,
        width: UiCoord,
        height: UiCoord,
    ) -> Response<()> {
        let id = self.begin_widget();
        let layer = self.ui.layer;

        let width = width.as_abolute(self.ui.bounds.width());
        let height = height.as_abolute(self.ui.bounds.height());

        let rect = layout_rect(RectLayoutDescriptor {
            width,
            height,
            padding: Some(self.theme.padding),
            dir: self.ui.layout_dir,
            bounds: self.ui.bounds,
        });

        self.image_rect(rect.min_x, rect.min_y, width, height, image, layer);
        self.submit_rect(id, rect, self.theme.padding);

        Response {
            id,
            hovered: self.is_hovered(id),
            active: self.is_active(id),
            inner: (),
            rect,
        }
    }

    pub fn label(&mut self, label: impl Into<String>) -> Response<()> {
        let id = self.begin_widget();
        let layer = self.ui.layer;
        let label = label.into();

        // shape the text
        // shape it at the origin and translate later, when the full rect is layouted
        let mut w = 0;
        let mut h = 0;
        let mut text_y = 0;
        let mut line_height = 0;
        let text_rects = self.ui.text_rects.len();
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
                0,
                text_y,
                line_width,
                line_height,
                self.theme.secondary_color,
                layer + 1,
                handle,
            );
            text_y += line_height;
        }

        let text_padding = self.theme().text_padding as i32;

        let rect = layout_rect(RectLayoutDescriptor {
            padding: Some(self.theme.padding),
            width: w + 2 * text_padding,
            height: h + 2 * text_padding,
            dir: self.ui.layout_dir,
            bounds: self.ui.bounds,
        });

        self.submit_rect(id, rect, self.theme.padding);

        let offset = layout_rect(RectLayoutDescriptor {
            padding: Some(Padding::splat(text_padding)),
            width: w,
            height: h,
            dir: self.ui.layout_dir,
            bounds: rect,
        });

        for r in &mut self.ui.text_rects[text_rects..] {
            r.x += offset.min_x;
            r.y += offset.min_y;
        }

        Response {
            id,
            hovered: self.is_hovered(id),
            active: self.is_active(id),
            inner: (),
            rect,
        }
    }

    /// When a widget has been completed, submit its bounding rectangle
    fn submit_rect(&mut self, id: UiId, rect: UiRect, padding: impl Into<Option<Padding>>) {
        let [p_left, p_right, p_top, p_bot] = padding
            .into()
            .map(|p| p.as_abs(self.ui.bounds.width(), self.ui.bounds.height()))
            .unwrap_or_default();

        match self.ui.layout_dir {
            LayoutDirection::TopDown(_) => {
                self.ui.bounds.min_y = rect.max_y + p_bot;
            }
            LayoutDirection::LeftRight(_) => {
                self.ui.bounds.min_x = rect.max_x + p_right;
            }
            LayoutDirection::BottomUp(_) => {
                self.ui.bounds.max_y = rect.min_y - p_top;
            }
            LayoutDirection::RightLeft(_) => {
                self.ui.bounds.max_x = rect.min_x - p_left;
            }
            LayoutDirection::Center => { /*noop*/ }
        }
        self.ui.bounding_boxes.insert(id, rect);
        self.ui.rect_history.push(rect);
    }

    pub fn begin_widget(&mut self) -> UiId {
        let index = self.ui.widget_ids.len() as IdxType;

        let parent = self
            .ui
            .id_stack
            .last()
            .and_then(|i| self.ui.widget_ids.get(*i as usize))
            .copied()
            .unwrap_or_else(|| {
                let mut id = UiId::SENTINEL;
                id.uid = self.ui.root_hash;
                id
            });

        let hash = fnv_1a(bytemuck::cast_slice(&[parent.uid, index]));
        let id = UiId {
            parent: parent.index,
            index,
            uid: hash,
            depth: parent.depth + 1,
        };
        self.ui.widget_ids.push(id);
        if let Some(i) = self.ui.id_stack.last_mut() {
            *i = id.index;
        }
        id
    }

    fn push_child(&mut self) {
        self.ui.id_stack.push(SENTINEL);
    }

    fn pop_child(&mut self) {
        self.ui.id_stack.pop();
    }

    pub fn button(&mut self, label: impl Into<String>) -> ButtonResponse {
        fn _button(this: &mut Ui, label: String) -> ButtonResponse {
            let id = this.begin_widget();
            let layer = this.ui.layer;

            let mut pressed = false;
            let contains_mouse = this.contains_mouse(id);
            let mut bg_color = this.theme.button_default.clone();
            let active = this.is_active(id);
            if active {
                bg_color = this
                    .theme
                    .button_pressed
                    .as_ref()
                    .unwrap_or(&this.theme.button_default)
                    .clone();
                if this.mouse_up() {
                    if this.is_hovered(id) {
                        pressed = true;
                    }
                    this.set_not_active(id);
                }
            } else if this.is_hovered(id) {
                bg_color = this
                    .theme
                    .button_hovered
                    .as_ref()
                    .unwrap_or(&this.theme.button_default)
                    .clone();
                if !contains_mouse {
                    this.set_not_hovered(id);
                } else if this.mouse_down() {
                    this.set_active(id);
                }
            }
            if contains_mouse {
                this.set_hovered(id);
            }

            // shape the text
            let mut w = 0;
            let mut h = 0;
            let mut text_y = 0;
            let text_color = this
                .theme
                .button_text_color
                .unwrap_or(this.theme.primary_color);

            let text_rect_idx = this.ui.text_rects.len();

            for line in label.split('\n').filter(|l| !l.is_empty()) {
                let (handle, e) =
                    this.shape_and_draw_line(line.to_owned(), this.theme.font_size as u32);
                let pic = &e.texture;
                let line_width = pic.width() as i32;
                let line_height = pic.height() as i32;
                h += line_height + 1;

                let mut delta = 0;
                if !active {
                    // add a shadow
                    this.text_rect(
                        0,
                        text_y + 1,
                        line_width,
                        line_height,
                        Color::BLACK,
                        layer + 1,
                        handle.clone(),
                    );
                } else {
                    // if active, then move the text into the shadow's position
                    // so it appears to have lowered
                    delta = 1
                }
                w = w.max(line_width + delta);
                this.text_rect(
                    delta,
                    text_y + delta,
                    line_width,
                    line_height,
                    text_color,
                    layer + 2,
                    handle,
                );
                text_y += line_height;
            }
            // background
            let text_padding = this.theme().text_padding as i32;

            let rect = layout_rect(RectLayoutDescriptor {
                padding: Some(this.theme.padding),
                width: w + 2 * text_padding,
                height: h + 2 * text_padding,
                dir: this.ui.layout_dir,
                bounds: this.ui.bounds,
            });

            this.submit_rect(id, rect, this.theme.padding);

            this.theme_rect(
                rect.min_x,
                rect.min_y,
                rect.width(),
                rect.height(),
                layer,
                bg_color,
            );

            let offset = layout_rect(RectLayoutDescriptor {
                padding: Some(Padding::splat(text_padding)),
                width: w,
                height: h,
                dir: LayoutDirection::TopDown(HorizontalAlignment::Center),
                bounds: rect,
            });

            for r in &mut this.ui.text_rects[text_rect_idx..] {
                r.x += offset.min_x;
                r.y += offset.min_y;
            }

            ButtonResponse {
                id,
                hovered: this.is_hovered(id),
                active,
                inner: ButtonState { pressed },
                rect,
            }
        }

        _button(self, label.into())
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
        let id = self.begin_widget();

        // bar
        let bounds = UiRect {
            min_x: scissor_bounds.max_x - scroll_bar_width,
            min_y: scissor_bounds.min_y,
            max_x: scissor_bounds.max_x,
            max_y: scissor_bounds.max_y,
        };
        self.color_rect_from_rect(bounds, 0xFF0000FF.into(), layer);
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
        } else if self.is_top_hovered(id) {
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
        self.color_rect_from_rect(control_box, 0xFF0AA0FF.into(), layer + 1);
        self.ui.bounding_boxes.insert(id, control_box);
    }

    fn horizontal_scroll_bar(
        &mut self,
        scissor_bounds: &UiRect,
        scroll_bar_height: i32,
        layer: u16,
        parent_state: &mut ScrollState,
    ) {
        let id = self.begin_widget();

        // bar
        let bounds = UiRect {
            min_x: scissor_bounds.min_x,
            min_y: scissor_bounds.max_y - scroll_bar_height,
            max_x: scissor_bounds.max_x,
            max_y: scissor_bounds.max_y,
        };
        self.color_rect_from_rect(bounds, 0xaaFF00FF.into(), layer);
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
        } else if self.is_top_hovered(id) {
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
        self.color_rect_from_rect(control_box, 0xFF0AA0FF.into(), layer + 1);
        self.ui.bounding_boxes.insert(id, control_box);
    }

    pub fn scroll_area(&mut self, desc: ScrollDescriptor, contents: impl FnMut(&mut Self)) {
        let id = self.begin_widget();
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
                self.set_hovered(id);
            }
            if self.is_hovered(id) {
                self.set_scrolling(id);
            } else {
                self.set_not_scrolling(id);
            }
            if self.is_scrolling(id) {
                let mut dt = 0.0;
                for ds in self.mouse.scroll.iter() {
                    // TODO: insert mouse events into ui_inputs
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
                        self.ui_inputs.keys.insert(KeyCode::ShiftLeft);
                        self.ui_inputs.keys.insert(KeyCode::ShiftRight);

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
                    *t = t.clamp(0.0, 1.0);
                }
            }
        }

        let offset_x = state.tx * state.scroll_width as f32;
        let offset_y = state.ty * state.scroll_height as f32;
        self.insert_memory(id, state);

        let old_bounds = self.ui.bounds;
        let scissor_bounds = layout_rect(RectLayoutDescriptor {
            width,
            height,
            padding: None,
            dir: self.ui.layout_dir,
            bounds: old_bounds,
        });

        let mut bounds = scissor_bounds;
        bounds.offset_x(-offset_x as i32);
        bounds.offset_y(-offset_y as i32);

        const BOUNDS_LIMIT: i32 = i32::MAX / 4;

        if desc.width.is_some() {
            match self.ui.layout_dir {
                LayoutDirection::LeftRight(_) => {
                    bounds.max_x = BOUNDS_LIMIT;
                }
                LayoutDirection::RightLeft(_) => {
                    bounds.min_x = -BOUNDS_LIMIT;
                }
                LayoutDirection::Center => {
                    bounds.min_x = -BOUNDS_LIMIT;
                    bounds.max_x = BOUNDS_LIMIT;
                }
                _ => {}
            }
        }
        if desc.height.is_some() {
            match self.ui.layout_dir {
                LayoutDirection::TopDown(_) => {
                    bounds.max_y = BOUNDS_LIMIT;
                }
                LayoutDirection::BottomUp(_) => {
                    bounds.min_y = -BOUNDS_LIMIT;
                }
                LayoutDirection::Center => {
                    bounds.min_y = -BOUNDS_LIMIT;
                    bounds.max_y = BOUNDS_LIMIT;
                }
                _ => {}
            }
        }

        self.ui.bounds = bounds;
        let scissor_idx = self.push_scissor(scissor_bounds);

        let layer = self.push_layer();
        let history_start = self.ui.rect_history.len();
        ///////////////////////
        self.children_content(contents);
        ///////////////////////
        let children_bounds = self.history_bounding_rect(history_start);

        let scroll_bar_size = self.theme.scroll_bar_size as i32;

        let state = self.get_memory_mut::<ScrollState>(id).unwrap();

        // compute the area of the scroll. Area = content bounds - viewport, so only the overlap is
        // counted
        state.scroll_width = children_bounds
            .width()
            .saturating_sub(width.saturating_sub(line_height as i32 + scroll_bar_size));
        state.scroll_height = children_bounds
            .height()
            .saturating_sub(height.saturating_sub(line_height as i32 + scroll_bar_size));
        let mut state = *state;

        if desc.width.is_some() {
            let mut scissor_bounds = scissor_bounds;
            if desc.height.is_some() {
                // prevent overlap
                scissor_bounds.max_x -= scroll_bar_size;
            }
            self.horizontal_scroll_bar(
                &scissor_bounds,
                scroll_bar_size,
                CONTEXT_LAYER - 1,
                &mut state,
            );
        }
        if desc.height.is_some() {
            self.vertical_scroll_bar(
                &scissor_bounds,
                scroll_bar_size,
                CONTEXT_LAYER - 1,
                &mut state,
            );
        }
        self.insert_memory(id, state);
        self.ui.bounds = old_bounds;
        self.submit_rect(id, scissor_bounds, self.theme.padding);

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

        let id = self.begin_widget();
        self.submit_rect(id, bounds, self.theme.padding);
        Response {
            id,
            hovered: self.is_hovered(id),
            active: self.is_active(id),
            rect: bounds,
            inner: (),
        }
    }

    pub fn drag_source(&mut self, mut contents: impl FnMut(&mut Self, &DragState)) -> DragResponse {
        let id = self.begin_widget();
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
                state.dragged = false;
                self.next_ids
                    .push(id, DRAG_LAYER)
                    .remove_flag(InteractionFlag::Dragged);
            } else {
                let drag_anchor = state.drag_anchor;
                let drag_start = state.drag_start;

                let offset = IVec2::new(
                    (self.mouse.cursor_position.x - drag_start.x) as i32,
                    (self.mouse.cursor_position.y - drag_start.y) as i32,
                );

                if offset.length_squared() > 5 {
                    state.dragged = true;
                }
                if state.dragged {
                    state.pos = drag_anchor + offset;
                    self.next_ids
                        .push(id, DRAG_LAYER)
                        .add_flag(InteractionFlag::Dragged);
                }
            }
        } else {
            state.pos = IVec2::new(old_bounds.min_x, old_bounds.min_y);
            if !self.is_anything_active() && self.contains_mouse(id) {
                self.set_hovered(id);
                if self.is_top_hovered(id) && self.mouse_down() {
                    is_being_dragged = true;
                    state.drag_start = self.mouse.cursor_position;
                    state.dragged = false;
                    self.set_active(id);
                }
            }
        }

        let history = std::mem::take(&mut self.ui.rect_history);
        let [p_left, p_right, p_top, p_bot] = self
            .theme
            .padding
            .as_abs(self.ui.bounds.width(), self.ui.bounds.height());
        self.ui.bounds = layout_rect(RectLayoutDescriptor {
            width: state.size.x,
            height: state.size.y,
            padding: Some(self.theme.padding),
            dir: self.ui.layout_dir,
            bounds: self.ui.bounds,
        });
        self.ui.bounds.move_to_x(state.pos.x);
        self.ui.bounds.move_to_y(state.pos.y);
        let last_scissor = self.ui.scissor_idx;
        let layer = self.ui.layer;
        if is_being_dragged {
            // Ensure that the widget is rendered on screen by pushing a new scissor that holds the
            // widget.
            // Only do this for the dragged widget, otherwise a lot of redundant scissors are
            // created.
            let mut scissor = self.ui.bounds;
            // undo padding
            scissor.min_x -= p_left;
            scissor.max_x += p_right;
            scissor.min_y -= p_top;
            scissor.max_y += p_bot;
            self.push_scissor(scissor);
            self.ui.layer = DRAG_LAYER;
        }
        self.ui.layer += 1;
        ///////////////////////
        self.children_content(|ui| {
            contents(ui, &state);
        });
        ///////////////////////
        self.ui.layer = layer;
        self.ui.bounds = old_bounds;
        let child_history = std::mem::replace(&mut self.ui.rect_history, history);
        let mut content_bounds = bounding_rect(&child_history);

        if is_being_dragged {
            self.color_rect_from_rect(content_bounds, self.theme.primary_color, layer);
            // move the content_bounds back to their origin, so they're submitted in their original
            // position, so the layout stays the same while dragging
            content_bounds.resize_w(state.size.x);
            content_bounds.resize_h(state.size.y);
            content_bounds.move_to_x(state.drag_anchor.x);
            content_bounds.move_to_y(state.drag_anchor.y);
        } else {
            self.ui.rect_history.extend_from_slice(&child_history);
            state.drag_anchor = IVec2::new(content_bounds.min_x, content_bounds.min_y);
            state.pos = state.drag_anchor;
            state.size = IVec2::new(content_bounds.width(), content_bounds.height());
        }
        self.ui.scissor_idx = last_scissor;

        self.submit_rect(id, content_bounds, self.theme.padding);

        self.insert_memory(id, state);

        DragResponse {
            is_being_dragged,
            inner: Response {
                id,
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

    pub fn on_next_layer(&mut self, mut contents: impl FnMut(&mut Self)) {
        let l = self.push_layer();
        contents(self);
        self.ui.layer = l;
    }

    pub fn drop_target(&mut self, mut contents: impl FnMut(&mut Self, DropState)) -> DropResponse {
        let id = self.begin_widget();
        let old_bounds = self.ui.bounds;
        let bg_layer = self.push_layer();
        let mut state = DropState::default();
        state.id = id;
        state.dragged = self.ids.dragged;

        if self.is_anything_dragged() {
            state.hovered = self.is_top_hovered(id);
            if state.hovered {
                if self.mouse_up() {
                    state.dropped = true;
                }
                if !self.contains_mouse(id) {
                    self.set_not_hovered(id);
                }
            }
            if self.contains_mouse(id) {
                self.set_hovered(id);
            }
        }

        let history_start = self.ui.rect_history.len();
        ///////////////////////
        self.children_content(|ui| {
            contents(ui, state);
        });
        ///////////////////////
        self.ui.bounds = old_bounds;

        let content_bounds = self.history_bounding_rect(history_start);
        self.submit_rect(id, content_bounds, self.theme.padding);

        let background = if state.hovered {
            self.theme.drop_target_hovered.as_ref()
        } else {
            Some(&self.theme.drop_target_default)
        };
        self.theme_rect(
            content_bounds.min_x,
            content_bounds.min_y,
            content_bounds.width(),
            content_bounds.height(),
            bg_layer,
            background
                .unwrap_or(&self.theme.drop_target_default)
                .clone(),
        );

        DropResponse {
            dropped: state.dropped,
            inner: Response {
                id,
                hovered: self.is_hovered(id),
                active: state.dropped,
                rect: content_bounds,
                inner: (),
            },
        }
    }

    pub fn input_string(&mut self, content: &mut String) -> Response<InputResponse> {
        self.input_string_impl(InputStringDescriptor {
            content,
            password: false,
        })
    }

    pub fn input_password(&mut self, content: &mut String) -> Response<InputResponse> {
        self.input_string_impl(InputStringDescriptor {
            content,
            password: true,
        })
    }

    fn input_string_impl(&mut self, desc: InputStringDescriptor) -> Response<InputResponse> {
        let id = self.begin_widget();
        let last_layer = self.push_layer();
        let layer = self.ui.layer;

        let mut state = self
            .get_memory_or_insert::<TextInputState>(id, || TextInputState {
                cursor: desc.content.len(),
                ..Default::default()
            })
            .clone();

        let mut changed = false;

        state.cursor = state.cursor.min(desc.content.len());
        let mouse_pos = self.mouse.cursor_position;

        // handle input
        let is_active = self.is_active(id);
        if is_active {
            self.ui_inputs.wants_keyboard = true;
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
                        self.ui_inputs.keys.insert(KeyCode::ArrowLeft);
                        cursor_update!({
                            if self.keyboard.pressed.contains(&KeyCode::ControlLeft)
                                || self.keyboard.pressed.contains(&KeyCode::ControlRight)
                            {
                                self.ui_inputs.keys.insert(KeyCode::ControlLeft);
                                self.ui_inputs.keys.insert(KeyCode::ControlRight);
                                state.cursor = 0;
                            } else {
                                state.cursor = state.cursor.saturating_sub(1);
                            }
                        });
                    }
                    KeyCode::ArrowRight => {
                        self.ui_inputs.keys.insert(KeyCode::ArrowRight);
                        cursor_update!({
                            if self.keyboard.pressed.contains(&KeyCode::ControlLeft)
                                || self.keyboard.pressed.contains(&KeyCode::ControlRight)
                            {
                                self.ui_inputs.keys.insert(KeyCode::ControlLeft);
                                self.ui_inputs.keys.insert(KeyCode::ControlRight);
                                state.cursor = desc.content.len();
                            } else {
                                state.cursor = desc.content.len().min(state.cursor + 1);
                            }
                        });
                    }
                    KeyCode::Home => {
                        self.ui_inputs.keys.insert(KeyCode::Home);
                        cursor_update!({
                            state.cursor = 0;
                        });
                    }
                    KeyCode::End => {
                        self.ui_inputs.keys.insert(KeyCode::End);
                        cursor_update!({
                            state.cursor = desc.content.len();
                        });
                    }
                    KeyCode::Backspace => {
                        self.ui_inputs.keys.insert(KeyCode::Backspace);
                        cursor_update!({
                            if state.cursor > 0 {
                                state.cursor -= 1;
                                desc.content.remove(state.cursor);
                                changed = true;
                            }
                        });
                    }
                    KeyCode::Delete => {
                        self.ui_inputs.keys.insert(KeyCode::Delete);
                        cursor_update!({
                            if state.cursor < desc.content.len() {
                                desc.content.remove(state.cursor);
                                changed = true;
                            }
                        });
                    }
                    // TODO: ctrl + c, ctrl + v, ctrl + a, selecting with shift
                    _ => {
                        if let Some(text) = self
                            .keyboard
                            .events
                            .get(k)
                            .and_then(|ev| ev.logical_key.to_text())
                        {
                            self.ui_inputs.keys.insert(*k);
                            desc.content.insert_str(state.cursor, text);
                            changed = true;
                            state.cursor += text.len();
                        }
                    }
                }
            }
        } else if self.is_hovered(id) {
            if !self.contains_mouse(id) {
                self.set_not_hovered(id);
            } else if !self.is_anything_active() && self.mouse_down() {
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
        let [p_left, p_right, p_top, p_bot] = self
            .theme
            .padding
            .as_abs(self.ui.bounds.width(), self.ui.bounds.height());
        let text_padding = self.theme.text_padding as i32;
        let [x, y] = [x + p_left + text_padding, y + p_top + text_padding];
        let mut line_width = 0;
        let mut line_height = self.theme.font_size as i32 + 7; // FIXME: +7??
        if !desc.content.is_empty() {
            let mouse_up = self.mouse_up();

            let pl = if !desc.password {
                desc.content.clone()
            } else {
                let width = desc.content.len();
                format!("{:*>width$}", '*')
            };

            let (handle, e) = self.shape_and_draw_line(pl, self.theme.font_size as u32);
            let pic = &e.texture;
            line_width = pic.width() as i32;
            line_height = pic.height() as i32;

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
                        #[cfg(feature = "tracing")]
                        tracing::debug!("Setting cursor to {}", state.cursor);
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
        }
        if is_active && state.show_caret {
            // draw caret
            let t = state.cursor as f64 / desc.content.len() as f64;
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

        if is_active && self.mouse_up() && !self.contains_mouse(id) {
            self.set_not_active(id);
        }

        let w = w.max(self.theme.font_size as i32 * 10);
        let h = h.max(line_height);
        let mut rect = UiRect {
            min_x: x - text_padding,
            min_y: y - text_padding,
            max_x: x + w + text_padding,
            max_y: y + h + text_padding,
        };
        self.color_rect_from_rect(rect, self.theme.secondary_color, layer);
        rect.min_x -= p_left;
        rect.min_y -= p_top;
        rect.max_x += p_right;
        rect.max_y += p_bot;
        self.submit_rect(id, rect, self.theme.padding);
        self.ui.layer = last_layer;
        self.insert_memory(id, state);
        Response {
            hovered: self.is_hovered(id),
            active: self.is_active(id),
            inner: InputResponse { changed },
            rect,
            id,
        }
    }

    pub fn with_outline(
        &mut self,
        OutlineDescriptor {
            fill_color,
            outline_color,
            outline_radius,
        }: OutlineDescriptor,
        content: impl FnMut(&mut Self),
    ) {
        let id = self.begin_widget();
        let last_layer = self.push_layer();
        let layer = self.push_layer();
        let history_start = self.ui.rect_history.len();

        let [p_left, p_right, p_top, p_bot] = self
            .theme
            .padding
            .as_abs(self.ui.bounds.width(), self.ui.bounds.height());

        let r = outline_radius as i32;
        self.ui.bounds.min_x += p_left + r;
        self.ui.bounds.min_y += p_top + r;
        //////////////////
        self.children_content(content);
        //////////////////

        let mut rect = self.history_bounding_rect(history_start);
        rect.min_x -= p_left + r;
        rect.max_x += p_right + r;
        rect.min_y -= p_top + r;
        rect.max_y += p_bot + r;
        // TODO: would be nice to support this drawing mode in the renderer, currently it will draw
        // a lot of transparent pixels
        self.color_rect_from_rect_with_outline(
            rect,
            fill_color,
            layer,
            outline_color,
            outline_radius,
        );

        self.submit_rect(id, rect, self.theme.padding);
        self.ui.layer = last_layer;
    }

    fn theme_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        layer: u16,
        entry: ThemeEntry,
    ) {
        match entry {
            ThemeEntry::Color(c) => {
                self.color_rect(x, y, width, height, c, layer);
            }
            ThemeEntry::Image(handle) => {
                self.image_rect(x, y, width, height, handle, layer);
            }
        }
    }

    fn context_menu_from_response<'b>(
        &'b mut self,
        resp: Response<()>,
        mut context_menu: impl FnMut(&mut Self, &mut ContextMenuState) + 'b,
    ) -> ContextMenuResponse<'b, 'a> {
        let parent_id = resp.id;
        let contains_mouse = self.contains_mouse(parent_id);

        let id = self.begin_widget();

        let mut state = self
            .remove_memory::<ContextMenuState>(parent_id)
            .map(|x| *x)
            .unwrap_or_default();

        state.open = self.has_context_menu(parent_id);
        if state.open {
            let history_start = self.ui.rect_history.len();
            let old_layer = std::mem::replace(&mut self.ui.layer, CONTEXT_LAYER + 2);
            let old_bounds = self.ui.bounds;

            let outline_size = 2;
            let [p_left, p_right, p_top, p_bot] = self
                .theme
                .padding
                .as_abs(self.ui.bounds.width(), self.ui.bounds.height());
            let p_horizontal = p_left + p_right;
            let p_vertical = p_bot + p_top;

            let mut bounds = resp.rect;
            bounds.move_to_x(state.offset.x);
            bounds.move_to_y(state.offset.y);
            bounds.max_x = self.ui.scissors[0].max_x - p_horizontal - outline_size;
            bounds.max_y = self.ui.scissors[0].max_y - p_vertical - outline_size;

            let new_bounds = std::mem::replace(&mut self.ui.bounds, bounds);

            let scissor = self.push_scissor(UiRect {
                min_x: bounds.min_x - p_horizontal - outline_size,
                min_y: bounds.min_y - p_vertical - outline_size,
                max_x: bounds.max_x + p_horizontal + outline_size,
                max_y: bounds.max_y + p_vertical + outline_size,
            });

            ///////////////////////
            self.children_content(|ui| {
                context_menu(ui, &mut state);
            });
            ///////////////////////
            self.ui.bounds = new_bounds;

            let context_bounds = self.history_bounding_rect(history_start);

            let mut bounds = context_bounds;
            bounds = bounds.grow_over_point(
                context_bounds.min_x - p_left - outline_size,
                context_bounds.min_y - p_top - outline_size,
            );
            bounds = bounds.grow_over_point(
                context_bounds.max_x + p_right + outline_size,
                context_bounds.max_y + p_bot + outline_size,
            );
            self.submit_rect(id, bounds, self.theme.padding);
            if self.contains_mouse(id) {
                self.next_ids
                    .push(id, CONTEXT_LAYER)
                    .add_flag(InteractionFlag::Hovered);
            }

            self.color_rect(
                bounds.min_x,
                bounds.min_y,
                bounds.width(),
                bounds.height(),
                Color::BLACK,
                CONTEXT_LAYER,
            );
            self.theme_rect(
                context_bounds.min_x - p_left,
                context_bounds.min_y - p_top,
                context_bounds.width() + p_horizontal,
                context_bounds.height() + p_vertical,
                CONTEXT_LAYER + 1,
                self.theme.context_background.clone(),
            );

            if !self.mouse.pressed.is_empty() && !self.contains_mouse(id) {
                #[cfg(feature = "tracing")]
                tracing::debug!("Closing context menu over {parent_id:?}");
                state.open = false;
            }

            // do not count the context menu in parent widgets, when calculating bounds
            self.ui
                .rect_history
                .resize_with(history_start, || unreachable!());
            self.ui.scissor_idx = scissor;
            self.ui.layer = old_layer;
            self.ui.bounds = old_bounds;
        } else {
            if contains_mouse
                && self.mouse.just_released.contains(&MouseButton::Right)
                && !self.is_context_menu_open()
            {
                #[cfg(feature = "tracing")]
                tracing::debug!("Opening context menu over {parent_id:?}");
                state.open = true;
                state.offset = IVec2::new(
                    self.mouse.cursor_position.x as i32,
                    self.mouse.cursor_position.y as i32,
                );
            }
        }

        let open = state.open;
        let is_currently_open = self.has_context_menu(parent_id);
        if is_currently_open {
            if !open {
                self.next_ids
                    .push(parent_id, self.ui.layer)
                    .remove_flag(InteractionFlag::ContextMenu);
            }
        } else if open {
            self.next_ids
                .push(parent_id, self.ui.layer)
                .add_flag(InteractionFlag::ContextMenu);
        }
        self.insert_memory(parent_id, state);
        let resp = ContextMenuResponse {
            open,
            inner: resp,
            ui: self,
            id: parent_id,
        };

        resp
    }

    pub fn select<'b, T: Eq + AsRef<str> + 'b>(
        &mut self,
        label: &str,
        current: T,
        options: &'b [T],
    ) -> SelectResponse {
        let resp = self.button(format!("{label}: {}", current.as_ref()));

        let parent_id = resp.id;

        let id = self.begin_widget();

        let mut state = self
            .remove_memory::<ContextMenuState>(parent_id)
            .map(|x| *x)
            .unwrap_or_default();

        if resp.pressed() {
            self.set_context_menu(resp.id);
        }
        state.offset = IVec2::new(resp.rect.min_x, resp.rect.max_y);

        state.open = self.has_context_menu(parent_id);
        let mut selected = None;
        if state.open {
            let history_start = self.ui.rect_history.len();
            let old_layer = std::mem::replace(&mut self.ui.layer, CONTEXT_LAYER + 2);
            let old_bounds = self.ui.bounds;

            let outline_size = 2;
            let [p_left, p_right, p_top, p_bot] = self
                .theme
                .padding
                .as_abs(self.ui.bounds.width(), self.ui.bounds.height());
            let p_horizontal = p_left + p_right;
            let p_vertical = p_bot + p_top;

            let mut bounds = resp.rect;
            bounds.move_to_x(state.offset.x);
            bounds.move_to_y(state.offset.y);
            bounds.max_x = self.ui.scissors[0].max_x - p_horizontal - outline_size;
            bounds.max_y = self.ui.scissors[0].max_y - p_vertical - outline_size;

            let new_bounds = std::mem::replace(&mut self.ui.bounds, bounds);

            let scissor = self.push_scissor(UiRect {
                min_x: bounds.min_x - p_horizontal - outline_size,
                min_y: bounds.min_y - p_vertical - outline_size,
                max_x: bounds.max_x + p_horizontal + outline_size,
                max_y: bounds.max_y + p_vertical + outline_size,
            });

            ///////////////////////
            self.children_content(|ui| {
                ui.vertical(None, |ui| {
                    // TODO: highlight if matches current
                    for (i, t) in options.iter().enumerate() {
                        if ui.button(t.as_ref()).pressed() {
                            selected = Some(i);
                            state.open = false;
                        }
                    }
                });
            });
            ///////////////////////
            self.ui.bounds = new_bounds;

            let context_bounds = self.history_bounding_rect(history_start);

            let mut bounds = context_bounds;
            bounds = bounds.grow_over_point(
                context_bounds.min_x - p_left - outline_size,
                context_bounds.min_y - p_top - outline_size,
            );
            bounds = bounds.grow_over_point(
                context_bounds.max_x + p_right + outline_size,
                context_bounds.max_y + p_bot + outline_size,
            );
            self.submit_rect(id, bounds, self.theme.padding);
            if self.contains_mouse(id) {
                self.next_ids
                    .push(id, CONTEXT_LAYER)
                    .add_flag(InteractionFlag::Hovered);
            }

            self.color_rect(
                bounds.min_x,
                bounds.min_y,
                bounds.width(),
                bounds.height(),
                Color::BLACK,
                CONTEXT_LAYER,
            );
            self.theme_rect(
                context_bounds.min_x - p_left,
                context_bounds.min_y - p_top,
                context_bounds.width() + p_horizontal,
                context_bounds.height() + p_vertical,
                CONTEXT_LAYER + 1,
                self.theme.context_background.clone(),
            );

            if !self.mouse.pressed.is_empty() && !self.contains_mouse(id) {
                #[cfg(feature = "tracing")]
                tracing::debug!("Closing context menu over {parent_id:?}");
                state.open = false;
            }

            // do not count the context menu in parent widgets, when calculating bounds
            self.ui
                .rect_history
                .resize_with(history_start, || unreachable!());
            self.ui.scissor_idx = scissor;
            self.ui.layer = old_layer;
            self.ui.bounds = old_bounds;
        }

        let open = state.open;
        let is_currently_open = self.has_context_menu(parent_id);
        if is_currently_open {
            if !open {
                self.next_ids
                    .push(parent_id, self.ui.layer)
                    .remove_flag(InteractionFlag::ContextMenu);
            }
        } else if open {
            self.next_ids
                .push(parent_id, self.ui.layer)
                .add_flag(InteractionFlag::ContextMenu);
        }
        self.insert_memory(parent_id, state);
        let resp = SelectResponse {
            inner: resp.map_unit(),
            selected,
        };

        resp
    }

    pub fn context_menu<'b>(
        &'b mut self,
        contents: impl FnMut(&mut Self) + 'b,
        context_menu: impl FnMut(&mut Self, &mut ContextMenuState) + 'b,
    ) -> ContextMenuResponse<'b, 'a> {
        let id = self.begin_widget();

        let history_start = self.ui.rect_history.len();
        ///////////////////////
        self.children_content(contents);
        ///////////////////////
        let content_bounds = self.submit_rect_group(id, history_start);

        let resp = Response {
            hovered: self.is_hovered(id),
            active: false, // the root element is never active
            rect: content_bounds,
            inner: (),
            id,
        };

        self.context_menu_from_response(resp, context_menu)
    }

    /// pos is the offset of the context_menu in screen space.
    /// if pos is None, then the context menu is opened at the cursor position
    pub fn open_context_menu(&mut self, id: UiId, pos: Option<IVec2>) {
        let offset = pos.unwrap_or_else(|| {
            let cur_pos = self.mouse.cursor_position;
            IVec2::new(cur_pos.x as i32, cur_pos.y as i32)
        });
        let state = self.get_memory_or_default::<ContextMenuState>(id);
        state.open = true;
        state.offset = offset;
        self.next_ids
            .push(id, CONTEXT_LAYER)
            .add_flag(InteractionFlag::ContextMenu);
    }

    pub fn close_context_menu(&mut self, id: UiId) {
        if let Some(m) = self.get_memory_mut::<ContextMenuState>(id) {
            m.open = false;
            self.next_ids
                .push(id, self.ui.layer)
                .remove_flag(InteractionFlag::ContextMenu);
        }
    }

    pub fn allocate_area(
        &mut self,
        width: UiCoord,
        height: UiCoord,
        contents: impl FnMut(&mut Self),
    ) {
        let id = self.begin_widget();
        let bounds = self.ui.bounds;

        let width = width.as_abolute(bounds.width());
        let height = height.as_abolute(bounds.height());

        self.ui.bounds.resize_w(width);
        self.ui.bounds.resize_h(height);
        let min_x = self.ui.bounds.min_x;
        let min_y = self.ui.bounds.min_y;
        self.ui.bounds.offset_x(bounds.min_x - min_x);
        self.ui.bounds.offset_y(bounds.min_y - min_y);

        let history_start = self.ui.rect_history.len();

        self.children_content(contents);

        self.ui.bounds = bounds;

        let bounds = self.history_bounding_rect(history_start);
        self.submit_rect(id, bounds, self.theme.padding);
    }

    /// Add a margin around the inner contents
    pub fn margin(&mut self, m: Padding, contents: impl FnMut(&mut Self)) {
        let id = self.begin_widget();
        let bounds = self.ui.bounds;

        let [left, right, top, bottom] = m.as_abs(bounds.width(), bounds.height());

        self.ui.bounds.min_x += left;
        self.ui.bounds.max_x -= right;
        self.ui.bounds.min_y += top;
        self.ui.bounds.max_y -= bottom;

        self.ui.bounds = layout_rect(RectLayoutDescriptor {
            width: self.ui.bounds.width(),
            height: self.ui.bounds.height(),
            padding: Some(m),
            dir: self.ui.layout_dir,
            bounds,
        });

        let history_start = self.ui.rect_history.len();

        self.children_content(contents);

        self.ui.bounds = bounds;

        let bounds = self.history_bounding_rect(history_start);
        self.submit_rect(id, bounds, m);
    }

    /// Add background to the widget. If background is None, then the Theme background is used
    pub fn background(&mut self, background: Option<ThemeEntry>, contents: impl FnMut(&mut Self)) {
        let id = self.begin_widget();
        let history_start = self.ui.rect_history.len();
        let layer = self.push_layer();

        self.children_content(contents);

        self.ui.layer = layer;

        let bounds = self.history_bounding_rect(history_start);
        self.theme_rect(
            bounds.min_x,
            bounds.min_y,
            bounds.width(),
            bounds.height(),
            layer,
            background.unwrap_or_else(|| self.theme.background.clone()),
        );
        self.submit_rect(id, bounds, self.theme.padding);
    }

    fn children_content(&mut self, mut contents: impl FnMut(&mut Self)) {
        let layer = self.push_layer();
        self.push_child();
        contents(self);
        self.pop_child();
        self.ui.layer = layer;
    }

    pub fn tooltip(&mut self, desc: TooltipDescriptor) {
        let bounds = self.ui.scissors[0];
        let old_bounds = std::mem::replace(&mut self.ui.bounds, bounds);
        let old_scissor = std::mem::replace(&mut self.ui.scissor_idx, 0);
        let old_layer = std::mem::replace(&mut self.ui.layer, CONTEXT_LAYER);

        self.ui.bounds.min_x = desc.x;
        self.ui.bounds.min_y = desc.y;

        ///////////////
        self.with_outline(
            OutlineDescriptor {
                fill_color: match self.theme().context_background {
                    ThemeEntry::Color(color) => color,
                    ThemeEntry::Image(_) => self.theme().secondary_color,
                },
                outline_color: Color::BLACK,
                outline_radius: 1,
            },
            |ui| {
                ui.label(desc.label);
            },
        );
        ///////////////

        self.ui.bounds = old_bounds;
        self.ui.scissor_idx = old_scissor;
        self.ui.layer = old_layer;
    }

    pub fn with_tooltip(&mut self, contents: impl FnMut(&mut Self), label: &str) {
        let id = self.begin_widget();
        let history_start = self.ui.rect_history.len();
        self.children_content(contents);
        let bounds = self.history_bounding_rect(history_start);

        self.submit_rect(id, bounds, self.theme.padding);

        let mut state = std::mem::take(self.get_memory_or_default::<TooltipState>(id));

        if self.contains_mouse(id) {
            state.hovered_seconds += self.delta_time.0.as_secs_f32();
            if state.hovered_seconds > 1.2 {
                let mouse = self.mouse.cursor_position;
                self.children_content(|ui| {
                    ui.tooltip(TooltipDescriptor {
                        x: mouse.x as i32,
                        y: mouse.y as i32 + 10,
                        label,
                    });
                });
            }
        } else {
            state.hovered_seconds = 0.0;
        }

        self.insert_memory(id, state);
    }
}

fn layout_rect(desc: RectLayoutDescriptor) -> UiRect {
    let bounds = desc.bounds;
    let mut rect = UiRect {
        min_x: bounds.min_x,
        min_y: bounds.min_y,
        max_x: bounds.min_x + desc.width,
        max_y: bounds.min_y + desc.height,
    };
    let [p_left, p_right, p_top, p_bot] = desc
        .padding
        .map(|p| p.as_abs(bounds.width(), bounds.height()))
        .unwrap_or_default();

    match desc.dir {
        LayoutDirection::TopDown(hor) => {
            rect.min_y = bounds.min_y + p_top;
            rect.max_y = (rect.min_y + desc.height).min(bounds.max_y - p_bot);
            rect = aling_horizontal(hor, rect, bounds);
        }
        LayoutDirection::BottomUp(hor) => {
            rect.max_y = bounds.max_y - p_bot;
            rect.min_y = (rect.max_y - desc.height).max(bounds.min_y + p_top);
            rect = aling_horizontal(hor, rect, bounds);
        }
        LayoutDirection::LeftRight(ver) => {
            rect.min_x = bounds.min_x + p_left;
            rect.max_x = (rect.min_x + desc.width).min(bounds.max_x - p_right);
            rect = aling_vertical(ver, rect, bounds);
        }
        LayoutDirection::RightLeft(ver) => {
            rect.max_x = bounds.max_x - p_right;
            rect.min_x = (rect.max_x - desc.width).max(bounds.min_x + p_left);
            rect = aling_vertical(ver, rect, bounds);
        }
        LayoutDirection::Center => {
            rect = aling_vertical(VerticalAlignment::Center, rect, bounds);
            rect = aling_horizontal(HorizontalAlignment::Center, rect, bounds);
        }
    }

    rect
}

fn aling_horizontal(alignment: HorizontalAlignment, mut rect: UiRect, bounds: UiRect) -> UiRect {
    let delta = match alignment {
        HorizontalAlignment::Left => bounds.min_x - rect.min_x,
        HorizontalAlignment::Right => bounds.max_x - rect.max_x,
        HorizontalAlignment::Center => bounds.center_x() - rect.center_x(),
    };
    rect.offset_x(delta);
    rect
}

fn aling_vertical(alignment: VerticalAlignment, mut rect: UiRect, bounds: UiRect) -> UiRect {
    let delta = match alignment {
        VerticalAlignment::Top => bounds.min_y - rect.min_y,
        VerticalAlignment::Center => bounds.max_y - rect.max_y,
        VerticalAlignment::Bottom => bounds.center_y() - rect.center_y(),
    };
    rect.offset_y(delta);
    rect
}

pub struct RectLayoutDescriptor {
    pub width: i32,
    pub height: i32,
    pub padding: Option<Padding>,
    pub dir: LayoutDirection,
    pub bounds: UiRect,
}

pub struct TooltipDescriptor<'a> {
    pub x: i32,
    pub y: i32,
    pub label: &'a str,
}

#[derive(Debug, Default)]
pub struct TooltipState {
    pub hovered_seconds: f32,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Padding {
    pub left: Option<UiCoord>,
    pub right: Option<UiCoord>,
    pub top: Option<UiCoord>,
    pub bottom: Option<UiCoord>,
}

impl Padding {
    /// Combine two padding structs. If a given padding field is Some in `self`, then it is selected,
    /// else the corresponding field in `other` is selected
    pub fn or(self, other: Padding) -> Self {
        Self {
            left: self.left.or(other.left),
            right: self.right.or(other.right),
            top: self.top.or(other.top),
            bottom: self.bottom.or(other.bottom),
        }
    }

    pub fn splat(p: impl Into<UiCoord>) -> Self {
        let p = p.into();
        Padding {
            left: Some(p),
            right: Some(p),
            top: Some(p),
            bottom: Some(p),
        }
    }

    pub fn horizontal(c: impl Into<UiCoord>) -> Self {
        let c = c.into();
        Padding {
            left: Some(c),
            right: Some(c),
            ..Default::default()
        }
    }

    pub fn vertical(c: impl Into<UiCoord>) -> Self {
        let c = c.into();
        Padding {
            top: Some(c),
            bottom: Some(c),
            ..Default::default()
        }
    }

    /// return left,right,top,bottom
    pub fn as_abs(self, max_horizontal: i32, max_vertical: i32) -> [i32; 4] {
        [
            self.left.map(|c| c.as_abolute(max_horizontal)).unwrap_or(0),
            self.right
                .map(|c| c.as_abolute(max_horizontal))
                .unwrap_or(0),
            self.top.map(|c| c.as_abolute(max_vertical)).unwrap_or(0),
            self.bottom.map(|c| c.as_abolute(max_vertical)).unwrap_or(0),
        ]
    }
}

pub struct ContextMenuResponse<'a, 'b> {
    pub open: bool,
    pub inner: Response<()>,
    ui: &'a mut Ui<'b>,
    id: UiId,
}

impl<'a, 'b> ContextMenuResponse<'a, 'b> {
    pub fn close(&mut self) {
        if self.open {
            let mem: &mut ContextMenuState = self.ui.get_memory_or_default(self.id);
            mem.open = false;
        }
    }
}

#[derive(Debug, Default)]
pub struct ContextMenuState {
    pub open: bool,
    pub offset: IVec2,
}

#[derive(Debug, Default)]
pub struct DragState {
    pub drag_start: PhysicalPosition<f64>,
    pub drag_anchor: IVec2,
    pub pos: IVec2,
    pub size: IVec2,
    pub dragged: bool,
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
    depth: IdxType,
}

impl UiId {
    pub const SENTINEL: UiId = Self {
        parent: SENTINEL,
        index: SENTINEL,
        uid: SENTINEL,
        depth: 0,
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
    pub id: UiId,
}

impl<T> Response<T> {
    pub fn as_ref(&self) -> Response<&T> {
        Response {
            hovered: self.hovered,
            active: self.active,
            rect: self.rect,
            id: self.id,
            inner: &self.inner,
        }
    }

    pub fn as_mut(&mut self) -> Response<&mut T> {
        Response {
            hovered: self.hovered,
            active: self.active,
            rect: self.rect,
            id: self.id,
            inner: &mut self.inner,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Response<U> {
        Response {
            hovered: self.hovered,
            active: self.active,
            rect: self.rect,
            id: self.id,
            inner: f(self.inner),
        }
    }

    pub fn map_unit(&self) -> Response<()> {
        Response {
            hovered: self.hovered,
            active: self.active,
            rect: self.rect,
            id: self.id,
            inner: (),
        }
    }

    pub fn context_menu<'a, 'b>(
        &self,
        ui: &'b mut Ui<'a>,
        context_menu: impl FnMut(&mut Ui<'a>, &mut ContextMenuState) + 'b,
    ) -> ContextMenuResponse<'b, 'a> {
        ui.context_menu_from_response(self.map_unit(), context_menu)
    }
}

pub type ButtonResponse = Response<ButtonState>;

impl ButtonResponse {
    pub fn pressed(&self) -> bool {
        self.inner.pressed
    }
}

#[derive(Debug, Clone, Copy)]
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
        let layer = ctx.ui.layer;
        ctx.ui.layer += 1;
        ctx.push_child();
        let history_start = ctx.ui.rect_history.len();

        ///////////////////////
        contents(ctx);
        ///////////////////////

        // restore state
        ctx.pop_child();
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

fn begin_frame(
    mut ui: ResMut<UiState>,
    window_size: Res<crate::renderer::WindowSize>,
    mut inputs: ResMut<UiInputs>,
) {
    ui.layout_dir = LayoutDirection::TopDown(HorizontalAlignment::Left);
    ui.root_hash = 0;
    ui.id_stack.clear();
    ui.widget_ids.clear();
    ui.rect_history.clear();
    ui.color_rects.clear();
    ui.text_rects.clear();
    ui.texture_rects.clear();
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
    inputs.clear();
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

// preserve the buffers by zipping together a query with the chunks, spawn new if not enough,
// GC if too many
// most frames should have the same items
fn submit_frame_texture_rects(
    mut ui: ResMut<UiState>,
    mut cmd: Commands,
    mut texture_rect_q: Query<(&mut TextureRectRequests, &mut UiScissor, EntityId)>,
) {
    let mut textured_rects = std::mem::take(&mut ui.texture_rects);
    textured_rects.sort_unstable_by_key(|r| r.scissor);

    let mut buffers_reused = 0;
    let mut rects_consumed = 0;
    for (g, (rects, sc, _id)) in
        (textured_rects.chunk_by_mut(|a, b| a.scissor == b.scissor)).zip(texture_rect_q.iter_mut())
    {
        buffers_reused += 1;
        rects_consumed += g.len();
        *sc = UiScissor(ui.scissors[g[0].scissor as usize]);
        rects.0.clear();
        rects.0.extend(g.iter_mut().map(|x| std::mem::take(x)));
    }
    for (_, _, id) in texture_rect_q.iter().skip(buffers_reused) {
        cmd.delete(id);
    }
    for g in textured_rects[rects_consumed..].chunk_by_mut(|a, b| a.scissor == b.scissor) {
        cmd.spawn().insert_bundle((
            UiScissor(ui.scissors[g[0].scissor as usize]),
            TextureRectRequests(g.iter_mut().map(|x| std::mem::take(x)).collect()),
        ));
    }
    ui.texture_rects = textured_rects;
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
    tick: Res<'a, Tick>,
    ui_inputs: ResMut<'a, UiInputs>,
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

pub const WINDOW_LAYER: u16 = 100;
pub const CONTEXT_LAYER: u16 = 10000;
pub const DRAG_LAYER: u16 = 10000;

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
        self.0.theme_rect(
            bounds.min_x,
            bounds.min_y,
            width + padding * 2,
            height + padding * 2,
            WINDOW_LAYER,
            self.theme().window_background.clone(),
        );
        self.0.push_child();
        ///////////////////////
        // Title
        {
            self.0.ui.bounds = title_bounds;
            self.0.push_scissor(title_bounds);
            self.0.label(desc.name);
            let title_id = self.0.begin_widget();
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
            self.0
                .submit_rect(title_id, title_bounds, self.0.theme.padding);
            self.0
                .color_rect_from_rect(title_bounds, Color::from_rgb(0x00ffff), WINDOW_LAYER);
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
        self.0.pop_child();
        self.0.ui.bounds = old_bounds;
        self.0.ui.scissor_idx = scissor;

        let r = self.0.history_bounding_rect(history_start);

        let state: &mut WindowState = self.0.ui.windows.get_mut(desc.name).unwrap();
        state.content_size = IVec2::new(r.width(), r.height());
        state.size = state.content_size + 2 * IVec2::splat(padding);
        state.size.y = (state.size.y).max(5) + self.0.theme.window_title_height as i32;
        self.0.ui.window_allocator = allocator;
    }

    pub fn panel(&mut self, desc: PanelDescriptor, contents: impl FnMut(&mut Ui)) {
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
            0,
            bounds.min_x,
            bounds.min_y,
            width,
            height,
        ]));
        self.0.ui.bounds = bounds;
        let scissor = self.0.push_scissor(bounds);

        let old_layer = self.0.push_layer();
        self.0.theme_rect(
            bounds.min_x,
            bounds.min_y,
            width,
            height,
            self.0.ui.layer,
            self.0.theme.background.clone(),
        );

        ///////////////////////
        self.0.children_content(contents);
        ///////////////////////
        self.0.ui.layer = old_layer;
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

    pub fn with_theme_override(
        &mut self,
        theme: ThemeOverride,
        mut contents: impl FnMut(&mut Self),
    ) {
        let theme = theme.apply(&mut self.0.theme);

        ///////////////////////
        contents(self);
        ///////////////////////

        theme.apply(&mut self.0.theme);
    }

    /// key should be a unique index for each empty call in an application
    pub fn empty(&mut self, key: i32, contents: impl FnMut(&mut Ui)) {
        let old_bounds = self.0.ui.bounds;
        self.0.ui.root_hash = fnv_1a(bytemuck::cast_slice(&[1, key]));
        let scissor = self.0.push_scissor(old_bounds);
        let old_layer = self.0.push_layer();

        ///////////////////////
        self.0.children_content(contents);
        ///////////////////////
        self.0.ui.layer = old_layer;
        self.0.ui.bounds = old_bounds;
        self.0.ui.scissor_idx = scissor;
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
        let tick = Res::new(db);
        let keys = ResMut::new(db);
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
            tick,
            ui_inputs: keys,
        })
    }

    fn resources_mut(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<NextUiIds>());
        set.insert(TypeId::of::<UiState>());
        set.insert(TypeId::of::<TextTextureCache>());
        set.insert(TypeId::of::<Assets<ShapingResult>>());
        set.insert(TypeId::of::<Theme>());
        set.insert(TypeId::of::<UiMemory>());
        set.insert(TypeId::of::<UiInputs>());
    }

    fn resources_const(set: &mut std::collections::HashSet<TypeId>) {
        set.insert(TypeId::of::<Tick>());
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
    pub scroll_width: i32,
    pub scroll_height: i32,
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

fn draw_bounding_boxes(mut ui: UiRoot) {
    let mut boxes: Vec<_> =
        ui.0.ui
            .bounding_boxes
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
    boxes.sort_unstable_by_key(|(k, _)| *k);

    let ui = &mut ui.0;
    for (id, rect) in boxes.into_iter() {
        let (handle, e) = ui.shape_and_draw_line(format!("{}/{}", id.parent as i32, id.index), 12);
        let pic = &e.texture;
        let line_width = pic.width() as i32;
        let line_height = pic.height() as i32;
        ui.color_rect_with_outline(
            rect.min_x,
            rect.min_y,
            line_width,
            line_height,
            Color::from_rgba(0x0000008F),
            2,
            Color::BLACK,
            CONTEXT_LAYER + 100,
        );
        ui.text_rect(
            rect.min_x,
            rect.min_y,
            line_width,
            line_height,
            Color::WHITE,
            CONTEXT_LAYER + 101,
            handle,
        );
        ui.color_rect_with_outline(
            rect.min_x,
            rect.min_y,
            rect.width(),
            rect.height(),
            Color::from_rgba(0x00000000),
            2,
            Color::BLACK,
            CONTEXT_LAYER + 99,
        );
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

#[derive(Debug, Clone)]
pub struct InputResponse {
    pub changed: bool,
}

/// return the offset by which the rect was repositioned
pub fn align_rect(
    rect: &mut UiRect,
    bounds: &UiRect,
    horizontal: Option<HorizontalAlignment>,
    vertical: Option<VerticalAlignment>,
    padding: IVec2,
) -> IVec2 {
    let dx;
    match horizontal {
        Some(HorizontalAlignment::Left) => {
            let min_x = bounds.min_x + padding.x;
            dx = min_x - rect.min_x;
        }
        Some(HorizontalAlignment::Center) => {
            dx = bounds.center_x() - rect.center_x();
        }
        Some(HorizontalAlignment::Right) => {
            let max_x = bounds.max_x - padding.x;
            dx = max_x - rect.max_x;
        }
        None => dx = 0,
    }

    let dy;
    match vertical {
        Some(VerticalAlignment::Top) => {
            let min_y = bounds.min_y + padding.y;
            dy = min_y - rect.min_y;
        }
        Some(VerticalAlignment::Center) => {
            dy = bounds.center_y() - rect.center_y();
        }
        Some(VerticalAlignment::Bottom) => {
            let max_y = bounds.max_y - padding.y;
            dy = max_y - rect.max_y;
        }
        None => dy = 0,
    }

    rect.offset_x(dx);
    rect.offset_y(dy);

    IVec2::new(dx, dy)
}

struct InputStringDescriptor<'a> {
    content: &'a mut String,
    password: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct OutlineDescriptor {
    pub fill_color: Color,
    pub outline_color: Color,
    pub outline_radius: u32,
}

impl Default for OutlineDescriptor {
    fn default() -> Self {
        Self {
            fill_color: Color::TRANSPARENT,
            outline_color: Color::BLACK,
            outline_radius: 1,
        }
    }
}

#[derive(Debug)]
pub struct SelectResponse {
    pub inner: Response<()>,
    pub selected: Option<usize>,
}
