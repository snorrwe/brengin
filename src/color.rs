use std::str::FromStr;

#[cfg(test)]
mod tests;

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
