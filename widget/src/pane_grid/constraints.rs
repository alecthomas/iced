use crate::core::Size;

use super::Axis;

/// Size constraints for a pane in a [`PaneGrid`].
///
/// [`PaneGrid`]: super::PaneGrid
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraints {
    /// The minimum size of the pane.
    pub min: Size,

    /// The maximum size of the pane.
    pub max: Size,

    pass_through_width: bool,
    pass_through_height: bool,
}

impl Constraints {
    /// Creates pane constraints from minimum and maximum sizes.
    pub const fn new(min: Size, max: Size) -> Self {
        Self {
            min,
            max,
            pass_through_width: false,
            pass_through_height: false,
        }
    }

    /// Creates pane constraints with only a minimum size.
    pub const fn minimum(min: Size) -> Self {
        Self::new(min, Size::INFINITE)
    }

    /// Creates pane constraints with the same minimum and maximum size.
    pub const fn fixed(size: Size) -> Self {
        Self::new(size, size)
    }

    /// Makes the pane transparent to resize interactions on the given axis.
    ///
    /// The pane keeps its constrained size while dragging its boundary resizes
    /// the nearest non-pass-through panes on either side.
    pub const fn pass_through(mut self, axis: Axis) -> Self {
        match axis {
            Axis::Horizontal => self.pass_through_height = true,
            Axis::Vertical => self.pass_through_width = true,
        }
        self
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

        Self { min, max, ..self }
    }

    pub(crate) fn is_hidden(self) -> bool {
        self.max.width == 0.0 && self.max.height == 0.0
    }

    pub(crate) fn passes_resize(self, axis: Axis) -> bool {
        match axis {
            Axis::Horizontal => self.pass_through_height,
            Axis::Vertical => self.pass_through_width,
        }
    }

    pub(crate) fn for_resize(self, axis: Axis) -> Self {
        if self.passes_resize(axis) {
            Self::fixed(Size::ZERO)
        } else {
            self
        }
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
