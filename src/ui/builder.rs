use cecs::prelude::*;

use crate::Plugin;

use super::core::{DrawRect, RectRequests};

/// UI context object. Use this to builder your user interface
#[derive(Debug, Default)]
pub struct Ui {
    hovered: UiId,
    active: UiId,
    /// stack of parents in the ui tree
    id_stack: Vec<IdType>,

    rects: Vec<DrawRect>,
    anchor: [u32; 2],
}

const FONT_SIZE: u32 = 32;

impl Ui {
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
        contents(Columns {
            ctx: self,
            cols: columns,
        });
        self.id_stack.pop();
    }

    pub fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: u32) {
        self.rects.push(DrawRect {
            x,
            y,
            w: width,
            h: height,
            color,
        })
    }

    pub fn button(&mut self, label: impl Into<String>) -> ButtonResponse {
        let label = label.into();
        let w = label.len() as u32 * FONT_SIZE;
        let h = FONT_SIZE;
        let [x, y] = self.anchor;
        self.anchor[1] += h;

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

        // TODO: render the button
        // TODO: width height from label content
        self.rect(x, y, w, h, 0xfab387ff);

        ButtonResponse {
            inner: Response {
                hovered: self.hovered == id,
                active: self.active == id,
                inner: (),
                rect: Aabb { x, y, w, h },
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
    pub rect: Aabb,
    pub inner: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aabb {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
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
}

impl<'a> Columns<'a> {
    pub fn column(&mut self, i: u32, mut contents: impl FnMut(&mut Ui)) {
        assert!(i < self.cols);
        *self.ctx.id_stack.last_mut().unwrap() = i;
        contents(self.ctx);
    }
}

fn begin_frame(mut ui: ResMut<Ui>) {
    ui.rects.clear();
    ui.anchor = [0, 0];
}

fn submit_frame(mut ui: ResMut<Ui>, mut rects: Query<&mut RectRequests>) {
    if let Some(dst) = rects.single_mut() {
        std::mem::swap(&mut ui.rects, &mut dst.0);
    }
}

fn setup(mut cmd: Commands) {
    cmd.spawn().insert(RectRequests::default());
}

pub struct UiBuilderPlugin;

impl Plugin for UiBuilderPlugin {
    fn build(self, app: &mut crate::App) {
        app.insert_resource(Ui::default());
        app.add_startup_system(setup);
        app.with_stage(crate::Stage::PreUpdate, |s| {
            s.add_system(begin_frame);
        });
        app.with_stage(crate::Stage::PostUpdate, |s| {
            s.add_system(submit_frame);
        });
    }
}
