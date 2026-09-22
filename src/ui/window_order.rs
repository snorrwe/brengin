use super::CONTEXT_LAYER;

/// The first layer of the band that follows a window of `height` layers
/// starting at `base`.
///
/// Saturates below [`CONTEXT_LAYER`] rather than wrapping. This does not keep
/// windows below context menus: `window()` draws content starting at
/// `base + 2`, so a window whose band saturates at the top can still draw
/// over a context menu. What saturating does do is pack exhausted windows
/// together at the top instead of wrapping their bases back around to zero.
/// Reaching the saturation point takes upwards of a thousand windows.
pub fn next_base(base: u16, height: u16) -> u16 {
    base.saturating_add(height).min(CONTEXT_LAYER - 1)
}

/// Whether `layer` falls inside the band of `height` layers starting at `base`.
///
/// Used to ask "is the topmost hovered widget mine, or does another window
/// cover me?" without caring which of my own widgets won.
pub fn layer_in_band(layer: u16, base: u16, height: u16) -> bool {
    base <= layer && layer < next_base(base, height)
}

/// Back-to-front ordering of windows by name.
///
/// A window's index in `order` is its z index: index 0 is the backmost window.
#[derive(Debug, Default)]
pub struct WindowOrder {
    pub order: Vec<String>,
    /// Frontmost window pressed this frame, applied at the next `begin_frame`.
    pub raise: Option<(u16, String)>,
}

impl WindowOrder {
    /// The z index of `name`. An unknown window is appended, so it opens in
    /// front of every window already on screen.
    pub fn z_of(&mut self, name: &str) -> u16 {
        let index = match self.order.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                self.order.push(name.to_owned());
                self.order.len() - 1
            }
        };
        index as u16
    }

    /// Record a bring-to-front request. Only the frontmost candidate of a frame
    /// is kept, so the window nearest the viewer wins the click.
    pub fn raise(&mut self, z: u16, name: &str) {
        if self
            .raise
            .as_ref()
            .map(|(current, _)| *current < z)
            .unwrap_or(true)
        {
            self.raise = Some((z, name.to_owned()));
        }
    }

    pub fn apply_raise(&mut self) {
        let Some((_, name)) = self.raise.take() else {
            return;
        };
        let Some(index) = self.order.iter().position(|n| *n == name) else {
            // the window was closed between the press and this frame
            return;
        };
        let name = self.order.remove(index);
        self.order.push(name);
    }
}
