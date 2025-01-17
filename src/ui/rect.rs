use super::div_half_ceil;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UiRect {
    /// center x
    pub min_x: i32,
    /// center y
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl UiRect {
    /// center x, center y, full width/height
    pub fn from_pos_size(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            min_x: x - w / 2,
            min_y: y - h / 2,
            max_x: x + div_half_ceil(w),
            max_y: y + div_half_ceil(h),
        }
    }

    pub fn grow_over(self, other: UiRect) -> UiRect {
        let min_x = self.min_x.min(other.min_x);
        let min_y = self.min_y.min(other.min_y);
        let max_x = self.max_x.max(other.max_x);
        let max_y = self.max_y.max(other.max_y);
        UiRect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.min_x <= x && x <= self.max_x && self.min_y <= y && y <= self.max_y
    }

    pub fn center_x(&self) -> i32 {
        self.min_x + div_half_ceil(self.max_x - self.min_x)
    }

    pub fn center_y(&self) -> i32 {
        self.min_y + div_half_ceil(self.max_y - self.min_y)
    }

    pub fn width(&self) -> i32 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> i32 {
        self.max_y - self.min_y
    }

    pub fn shrink_x(&mut self, v: i32) {
        self.min_x += v;
        self.max_x -= v;

        if self.min_x > self.max_x {
            std::mem::swap(&mut self.max_x, &mut self.min_x);
        }
    }

    pub fn shrink_y(&mut self, v: i32) {
        self.min_y += v;
        self.max_y -= v;

        if self.min_y > self.max_y {
            std::mem::swap(&mut self.max_y, &mut self.min_y);
        }
    }

    pub fn grow_x(&mut self, v: i32) {
        self.shrink_x(-v);
    }

    pub fn grow_y(&mut self, v: i32) {
        self.shrink_y(-v);
    }

    pub fn offset_x(&mut self, d: i32) {
        self.min_x += d;
        self.max_x += d;
    }

    pub fn offset_y(&mut self, d: i32) {
        self.min_y += d;
        self.max_y += d;
    }

    pub fn move_to_x(&mut self, x: i32) {
        let delta = x - self.center_x();
        self.min_x += delta;
        self.max_x += delta;
    }

    pub fn move_to_y(&mut self, y: i32) {
        let delta = y - self.center_y();
        self.min_y += delta;
        self.max_y += delta;
    }
}
