use crate::core::Size;

/// Size constraints for a pane in a [`PaneGrid`].
///
/// [`PaneGrid`]: super::PaneGrid
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraints {
    /// The minimum size of the pane.
    pub min: Size,

    /// The maximum size of the pane.
    pub max: Size,
}

impl Constraints {
    /// Creates pane constraints from minimum and maximum sizes.
    pub const fn new(min: Size, max: Size) -> Self {
        Self { min, max }
    }

    /// Creates pane constraints with only a minimum size.
    pub const fn minimum(min: Size) -> Self {
        Self::new(min, Size::INFINITE)
    }

    /// Creates pane constraints with the same minimum and maximum size.
    pub const fn fixed(size: Size) -> Self {
        Self::new(size, size)
    }

    pub(crate) fn normalized(self) -> Self {
        let min = Size::new(
            normalize_min(self.min.width),
            normalize_min(self.min.height),
        );
        let max = Size::new(
            normalize_max(self.max.width, min.width),
            normalize_max(self.max.height, min.height),
        );

        Self { min, max }
    }

    pub(crate) fn is_hidden(self) -> bool {
        self.max.width == 0.0 && self.max.height == 0.0
    }
}

impl Default for Constraints {
    fn default() -> Self {
        Self::minimum(Size::ZERO)
    }
}

fn normalize_min(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn normalize_max(value: f32, min: f32) -> f32 {
    if value.is_nan() {
        f32::INFINITY
    } else {
        value.max(min)
    }
}
