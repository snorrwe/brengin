#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UiRect {
    /// center x
    pub x: u32,
    /// center y
    pub y: u32,
    pub w: u32,
    pub h: u32,
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
            x: (minx + halfw) as u32,
            y: (miny + halfh) as u32,
            w: w as u32,
            h: h as u32,
        }
    }

    #[inline]
    pub fn y_end(self) -> u32 {
        self.y + self.h
    }

    #[inline]
    pub fn x_end(self) -> u32 {
        self.x + self.w
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
