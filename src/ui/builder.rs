/// UI context object. Use this to builder your user interface
pub struct Ui {
    hovered: UiId,
    active: UiId,
    /// stack of parents in the ui tree
    id_stack: Vec<IdType>,
}

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
        assert!(self.id_stack.len() >= 1);
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
        // TODO:
        false
    }

    #[inline]
    fn is_hovered(&self, id: UiId) -> bool {
        // TODO:
        false
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

    pub fn button(&mut self, label: impl Into<String>) -> ButtonResponse {
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
        ButtonResponse {
            inner: Response {
                hovered: self.hovered == id,
                active: self.active == id,
                inner: (),
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
}

impl<'a> Columns<'a> {
    pub fn column(&mut self, i: u32, mut contents: impl FnMut(&mut Ui)) {
        assert!(i < self.cols);
        *self.ctx.id_stack.last_mut().unwrap() = i;
        contents(self.ctx);
    }
}
