/// UI context object. Use this to builder your user interface
pub struct Ui {
    hovered: UiId,
    active: UiId,
    /// stack of parents in the ui tree
    id_stack: Vec<IdType>,
}

impl Ui {
    fn set_hovered(&mut self, id: UiId) {
        self.hovered = id;
    }

    fn set_active(&mut self, id: UiId) {
        self.active = id;
    }

    fn parent(&self) -> IdType {
        if self.id_stack.len() >= 2 {
            self.id_stack[self.id_stack.len() - 2]
        } else {
            SENTINEL
        }
    }

    fn current_idx(&self) -> IdType {
        assert!(self.id_stack.len() >= 1);
        unsafe { *self.id_stack.last().unwrap_unchecked() }
    }

    fn current_id(&self) -> UiId {
        UiId {
            parent: self.parent(),
            index: self.current_idx(),
        }
    }

    fn is_active(&self, id: UiId) -> bool {
        // TODO:
        false
    }

    fn is_hovered(&self, id: UiId) -> bool {
        // TODO:
        false
    }

    fn mouse_up(&self) -> bool {
        // TODO:
        false
    }

    fn mouse_down(&self) -> bool {
        // TODO:
        false
    }

    fn contains_mouse(&self, id: UiId) -> bool {
        // TODO:
        false
    }

    fn set_not_active(&mut self, id: UiId) {
        if self.active == id {
            self.active = UiId::SENTINEL;
        }
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
        } else if self.is_hovered(id) {
            if self.mouse_down() {
                self.set_active(id);
            }
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
