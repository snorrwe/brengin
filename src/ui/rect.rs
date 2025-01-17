use super::div_half_ceil;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UiRect {
    /// center x
    pub x: i32,
    /// center y
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl UiRect {
    pub fn grow_over(self, other: UiRect) -> UiRect {
        let halfw = self.w as f64 * 0.5;
        let halfh = self.h as f64 * 0.5;
        let self_minx = self.x as f64 - halfw;
        let self_miny = self.y as f64 - halfh;
        let self_maxx = self.x as f64 + halfw;
        let self_maxy = self.y as f64 + halfh;
        let halfw = other.w as f64 * 0.5;
        let halfh = other.h as f64 * 0.5;
        let minx = self_minx.min(other.x as f64 - halfw);
        let miny = self_miny.min(other.y as f64 - halfh);
        let maxx = self_maxx.max(other.x as f64 + halfw);
        let maxy = self_maxy.max(other.y as f64 + halfh);

        let w = maxx - minx;
        let h = maxy - miny;

        let halfw = w * 0.5;
        let halfh = h * 0.5;

        UiRect {
            x: (minx + halfw) as i32,
            y: (miny + halfh) as i32,
            w: w as i32,
            h: h as i32,
        }
    }

    #[inline]
    pub fn y_start(self) -> i32 {
        self.y - self.h / 2
    }

    #[inline]
    pub fn x_start(self) -> i32 {
        self.x - self.w / 2
    }

    #[inline]
    pub fn y_end(self) -> i32 {
        self.y + div_half_ceil(self.h)
    }

    #[inline]
    pub fn x_end(self) -> i32 {
        self.x + div_half_ceil(self.w)
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let dx = x as i64 - self.x as i64;
        let dy = y as i64 - self.y as i64;

        0 <= dx && dx < div_half_ceil(self.w) as i64 && 0 <= dy && dy < div_half_ceil(self.h) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grow_over() {
        let a = UiRect {
            x: 0,
            y: 0,
            w: 4,
            h: 2,
        };
        let b = UiRect {
            x: 10,
            y: 10,
            w: 4,
            h: 3,
        };

        let c = a.grow_over(b);

        assert_eq!(c.w, 14);
        assert_eq!(c.h, 12);

        assert_eq!(c.x, 5);
        assert_eq!(c.y, 5);
    }
}
