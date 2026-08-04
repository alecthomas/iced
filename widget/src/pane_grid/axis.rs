use crate::core::Rectangle;

/// A fixed reference line for the measurement of coordinates.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Axis {
    /// The horizontal axis: —
    Horizontal,
    /// The vertical axis: |
    Vertical,
}

impl Axis {
    /// Splits the provided [`Rectangle`] on the current [`Axis`] with the
    /// given `ratio` and `spacing`.
    pub fn split(
        &self,
        rectangle: &Rectangle,
        ratio: f32,
        spacing: f32,
        min_size_a: f32,
        min_size_b: f32,
    ) -> (Rectangle, Rectangle, f32) {
        self.split_with_constraints(
            rectangle,
            ratio,
            spacing,
            min_size_a,
            f32::INFINITY,
            min_size_b,
            f32::INFINITY,
        )
    }

    pub(crate) fn split_with_constraints(
        self,
        rectangle: &Rectangle,
        ratio: f32,
        spacing: f32,
        min_size_a: f32,
        max_size_a: f32,
        min_size_b: f32,
        max_size_b: f32,
    ) -> (Rectangle, Rectangle, f32) {
        match self {
            Axis::Horizontal => {
                let (height_top, height_bottom, ratio) = split_extent(
                    rectangle.height,
                    ratio,
                    spacing,
                    min_size_a,
                    max_size_a,
                    min_size_b,
                    max_size_b,
                );

                (
                    Rectangle {
                        height: height_top,
                        ..*rectangle
                    },
                    Rectangle {
                        y: rectangle.y + height_top + spacing,
                        height: height_bottom,
                        ..*rectangle
                    },
                    ratio,
                )
            }
            Axis::Vertical => {
                let (width_left, width_right, ratio) = split_extent(
                    rectangle.width,
                    ratio,
                    spacing,
                    min_size_a,
                    max_size_a,
                    min_size_b,
                    max_size_b,
                );

                (
                    Rectangle {
                        width: width_left,
                        ..*rectangle
                    },
                    Rectangle {
                        x: rectangle.x + width_left + spacing,
                        width: width_right,
                        ..*rectangle
                    },
                    ratio,
                )
            }
        }
    }

    /// Calculates the bounds of the split line in a [`Rectangle`] region.
    pub fn split_line_bounds(&self, rectangle: Rectangle, ratio: f32, spacing: f32) -> Rectangle {
        match self {
            Axis::Horizontal => Rectangle {
                x: rectangle.x,
                y: (rectangle.y + rectangle.height * ratio - spacing / 2.0).round(),
                width: rectangle.width,
                height: spacing,
            },
            Axis::Vertical => Rectangle {
                x: (rectangle.x + rectangle.width * ratio - spacing / 2.0).round(),
                y: rectangle.y,
                width: spacing,
                height: rectangle.height,
            },
        }
    }
}

fn split_extent(
    extent: f32,
    ratio: f32,
    spacing: f32,
    min_a: f32,
    max_a: f32,
    min_b: f32,
    max_b: f32,
) -> (f32, f32, f32) {
    let extent = extent.max(0.0);
    let spacing = spacing.max(0.0).min(extent);
    let available = extent - spacing;
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let desired = (extent * ratio - spacing / 2.0).round();

    let lower = min_a.max(available - max_b);
    let upper = max_a.min(available - min_b);
    let size_a = if lower <= upper {
        desired.clamp(lower, upper)
    } else if min_a + min_b > available {
        let total_min = min_a + min_b;

        if total_min > 0.0 {
            available * min_a / total_min
        } else {
            available * ratio
        }
    } else {
        desired.clamp(min_a.min(available), (available - min_b).max(0.0))
    }
    .clamp(0.0, available);
    let size_b = available - size_a;
    let actual_ratio = if extent > 0.0 {
        (size_a + spacing / 2.0) / extent
    } else {
        0.5
    };

    (size_a, size_b, actual_ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Case {
        Horizontal {
            overall_height: f32,
            spacing: f32,
            top_height: f32,
            bottom_y: f32,
            bottom_height: f32,
        },
        Vertical {
            overall_width: f32,
            spacing: f32,
            left_width: f32,
            right_x: f32,
            right_width: f32,
        },
    }

    #[test]
    fn split() {
        let cases = vec![
            // Even height, even spacing
            Case::Horizontal {
                overall_height: 10.0,
                spacing: 2.0,
                top_height: 4.0,
                bottom_y: 6.0,
                bottom_height: 4.0,
            },
            // Odd height, even spacing
            Case::Horizontal {
                overall_height: 9.0,
                spacing: 2.0,
                top_height: 4.0,
                bottom_y: 6.0,
                bottom_height: 3.0,
            },
            // Even height, odd spacing
            Case::Horizontal {
                overall_height: 10.0,
                spacing: 1.0,
                top_height: 5.0,
                bottom_y: 6.0,
                bottom_height: 4.0,
            },
            // Odd height, odd spacing
            Case::Horizontal {
                overall_height: 9.0,
                spacing: 1.0,
                top_height: 4.0,
                bottom_y: 5.0,
                bottom_height: 4.0,
            },
            // Even width, even spacing
            Case::Vertical {
                overall_width: 10.0,
                spacing: 2.0,
                left_width: 4.0,
                right_x: 6.0,
                right_width: 4.0,
            },
            // Odd width, even spacing
            Case::Vertical {
                overall_width: 9.0,
                spacing: 2.0,
                left_width: 4.0,
                right_x: 6.0,
                right_width: 3.0,
            },
            // Even width, odd spacing
            Case::Vertical {
                overall_width: 10.0,
                spacing: 1.0,
                left_width: 5.0,
                right_x: 6.0,
                right_width: 4.0,
            },
            // Odd width, odd spacing
            Case::Vertical {
                overall_width: 9.0,
                spacing: 1.0,
                left_width: 4.0,
                right_x: 5.0,
                right_width: 4.0,
            },
        ];
        for case in cases {
            match case {
                Case::Horizontal {
                    overall_height,
                    spacing,
                    top_height,
                    bottom_y,
                    bottom_height,
                } => {
                    let a = Axis::Horizontal;
                    let r = Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: overall_height,
                    };
                    let (top, bottom, _ratio) = a.split(&r, 0.5, spacing, 0.0, 0.0);
                    assert_eq!(
                        top,
                        Rectangle {
                            height: top_height,
                            ..r
                        }
                    );
                    assert_eq!(
                        bottom,
                        Rectangle {
                            y: bottom_y,
                            height: bottom_height,
                            ..r
                        }
                    );
                }
                Case::Vertical {
                    overall_width,
                    spacing,
                    left_width,
                    right_x,
                    right_width,
                } => {
                    let a = Axis::Vertical;
                    let r = Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: overall_width,
                        height: 10.0,
                    };
                    let (left, right, _ratio) = a.split(&r, 0.5, spacing, 0.0, 0.0);
                    assert_eq!(
                        left,
                        Rectangle {
                            width: left_width,
                            ..r
                        }
                    );
                    assert_eq!(
                        right,
                        Rectangle {
                            x: right_x,
                            width: right_width,
                            ..r
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn split_respects_axis_constraints() {
        let rectangle = Rectangle::new(
            crate::core::Point::ORIGIN,
            crate::core::Size::new(800.0, 600.0),
        );
        let (left, right, ratio) = Axis::Vertical.split_with_constraints(
            &rectangle,
            0.5,
            4.0,
            26.0,
            26.0,
            300.0,
            f32::INFINITY,
        );

        assert_eq!(left.width, 26.0);
        assert_eq!(right.width, 770.0);
        assert_eq!(right.x, 30.0);
        assert_eq!(ratio, 0.035);
    }

    #[test]
    fn split_compresses_impossible_minimums_without_overflow() {
        let rectangle = Rectangle::new(
            crate::core::Point::ORIGIN,
            crate::core::Size::new(500.0, 600.0),
        );
        let (left, right, _) = Axis::Vertical.split_with_constraints(
            &rectangle,
            0.5,
            4.0,
            300.0,
            f32::INFINITY,
            300.0,
            f32::INFINITY,
        );

        assert_eq!(left.width, 248.0);
        assert_eq!(right.width, 248.0);
        assert_eq!(right.x + right.width, rectangle.width);
    }
}
