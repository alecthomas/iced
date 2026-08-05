use crate::core::{Rectangle, Size};
use crate::pane_grid::{Axis, Constraints, Pane, Split};

use std::collections::BTreeMap;

/// A layout node of a [`PaneGrid`].
///
/// [`PaneGrid`]: super::PaneGrid
#[derive(Debug, Clone)]
pub enum Node {
    /// The region of this [`Node`] is split into two.
    Split {
        /// The [`Split`] of this [`Node`].
        id: Split,

        /// The direction of the split.
        axis: Axis,

        /// The ratio of the split in [0.0, 1.0].
        ratio: f32,

        /// The left/top [`Node`] of the split.
        a: Box<Node>,

        /// The right/bottom [`Node`] of the split.
        b: Box<Node>,
    },
    /// The region of this [`Node`] is taken by a [`Pane`].
    Pane(Pane),
}

#[derive(Debug)]
enum Count {
    Split {
        horizontal: usize,
        vertical: usize,
        a: Box<Count>,
        b: Box<Count>,
    },
    Pane,
}

#[derive(Debug)]
enum ConstraintTree {
    Split {
        constraints: Constraints,
        a: Box<ConstraintTree>,
        b: Box<ConstraintTree>,
    },
    Pane(Constraints),
}

#[derive(Debug, Clone, Copy)]
enum Boundary {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Branch {
    A,
    B,
}

#[derive(Debug, Clone, Copy)]
struct ExtentConstraints {
    min: f32,
    max: f32,
}

#[derive(Debug, Clone, Copy)]
struct SplitSnapshot {
    a: Rectangle,
    b: Rectangle,
}

impl ConstraintTree {
    fn constraints(&self) -> Constraints {
        match self {
            Self::Split { constraints, .. } | Self::Pane(constraints) => *constraints,
        }
    }
}

impl Count {
    fn horizontal(&self) -> usize {
        match self {
            Count::Split { horizontal, .. } => *horizontal,
            Count::Pane => 0,
        }
    }

    fn vertical(&self) -> usize {
        match self {
            Count::Split { vertical, .. } => *vertical,
            Count::Pane => 0,
        }
    }
}

impl Node {
    /// Returns an iterator over each [`Split`] in this [`Node`].
    pub fn splits(&self) -> impl Iterator<Item = &Split> {
        let mut unvisited_nodes = vec![self];

        std::iter::from_fn(move || {
            while let Some(node) = unvisited_nodes.pop() {
                if let Node::Split { id, a, b, .. } = node {
                    unvisited_nodes.push(a);
                    unvisited_nodes.push(b);

                    return Some(id);
                }
            }

            None
        })
    }

    fn count(&self) -> Count {
        match self {
            Node::Split { a, b, axis, .. } => {
                let a = a.count();
                let b = b.count();

                let (horizontal, vertical) = match axis {
                    Axis::Horizontal => (
                        1 + a.horizontal() + b.horizontal(),
                        a.vertical().max(b.vertical()),
                    ),
                    Axis::Vertical => (
                        a.horizontal().max(b.horizontal()),
                        1 + a.vertical() + b.vertical(),
                    ),
                };

                Count::Split {
                    horizontal,
                    vertical,
                    a: Box::new(a),
                    b: Box::new(b),
                }
            }
            Node::Pane(_) => Count::Pane,
        }
    }

    fn constraints(&self, panes: &BTreeMap<Pane, Constraints>, spacing: f32) -> ConstraintTree {
        match self {
            Node::Split { axis, a, b, .. } => {
                let a = a.constraints(panes, spacing);
                let b = b.constraints(panes, spacing);
                let a_constraints = a.constraints();
                let b_constraints = b.constraints();
                let spacing = visible_spacing(*axis, spacing, a_constraints, b_constraints);
                let constraints = if a_constraints.is_hidden(*axis) {
                    b_constraints
                } else if b_constraints.is_hidden(*axis) {
                    a_constraints
                } else {
                    match axis {
                        Axis::Horizontal => Constraints::new(
                            Size::new(
                                a_constraints.min.width.max(b_constraints.min.width),
                                a_constraints.min.height + spacing + b_constraints.min.height,
                            ),
                            Size::new(
                                a_constraints.max.width.min(b_constraints.max.width),
                                a_constraints.max.height + spacing + b_constraints.max.height,
                            ),
                        ),
                        Axis::Vertical => Constraints::new(
                            Size::new(
                                a_constraints.min.width + spacing + b_constraints.min.width,
                                a_constraints.min.height.max(b_constraints.min.height),
                            ),
                            Size::new(
                                a_constraints.max.width + spacing + b_constraints.max.width,
                                a_constraints.max.height.min(b_constraints.max.height),
                            ),
                        ),
                    }
                }
                .normalized();

                ConstraintTree::Split {
                    constraints,
                    a: Box::new(a),
                    b: Box::new(b),
                }
            }
            Node::Pane(pane) => {
                ConstraintTree::Pane(panes.get(pane).copied().unwrap_or_default().normalized())
            }
        }
    }

    /// Returns the rectangular region for each [`Pane`] in the [`Node`] given
    /// the spacing between panes and the total available space.
    pub fn pane_regions(
        &self,
        spacing: f32,
        min_size: f32,
        bounds: Size,
    ) -> BTreeMap<Pane, Rectangle> {
        let mut regions = BTreeMap::new();
        let count = self.count();

        self.compute_regions(
            spacing,
            min_size,
            &Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.width,
                height: bounds.height,
            },
            &count,
            &mut regions,
        );

        regions
    }

    /// Returns the rectangular region for each [`Pane`] using pane-specific
    /// size constraints.
    pub fn pane_regions_with_constraints(
        &self,
        spacing: f32,
        constraints: &BTreeMap<Pane, Constraints>,
        bounds: Size,
    ) -> BTreeMap<Pane, Rectangle> {
        let mut regions = BTreeMap::new();
        let constraints = self.constraints(constraints, spacing);

        self.compute_constrained_regions(
            spacing,
            &Rectangle::new(crate::core::Point::ORIGIN, bounds),
            &constraints,
            &mut regions,
        );

        regions
    }

    /// Returns the axis, rectangular region, and ratio for each [`Split`] in
    /// the [`Node`] given the spacing between panes and the total available
    /// space.
    pub fn split_regions(
        &self,
        spacing: f32,
        min_size: f32,
        bounds: Size,
    ) -> BTreeMap<Split, (Axis, Rectangle, f32)> {
        let mut splits = BTreeMap::new();
        let count = self.count();

        self.compute_splits(
            spacing,
            min_size,
            &Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.width,
                height: bounds.height,
            },
            &count,
            &mut splits,
        );

        splits
    }

    /// Returns each split region using pane-specific size constraints.
    pub fn split_regions_with_constraints(
        &self,
        spacing: f32,
        constraints: &BTreeMap<Pane, Constraints>,
        bounds: Size,
    ) -> BTreeMap<Split, (Axis, Rectangle, f32)> {
        let mut splits = BTreeMap::new();
        let constraints = self.constraints(constraints, spacing);

        self.compute_constrained_splits(
            spacing,
            &Rectangle::new(crate::core::Point::ORIGIN, bounds),
            &constraints,
            &mut splits,
        );

        splits
    }

    pub(crate) fn find(&mut self, pane: Pane) -> Option<&mut Node> {
        match self {
            Node::Split { a, b, .. } => a.find(pane).or_else(move || b.find(pane)),
            Node::Pane(p) => {
                if *p == pane {
                    Some(self)
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn split(&mut self, id: Split, axis: Axis, new_pane: Pane) {
        *self = Node::Split {
            id,
            axis,
            ratio: 0.5,
            a: Box::new(self.clone()),
            b: Box::new(Node::Pane(new_pane)),
        };
    }

    pub(crate) fn split_inverse(&mut self, id: Split, axis: Axis, pane: Pane) {
        *self = Node::Split {
            id,
            axis,
            ratio: 0.5,
            a: Box::new(Node::Pane(pane)),
            b: Box::new(self.clone()),
        };
    }

    pub(crate) fn update(&mut self, f: &impl Fn(&mut Node)) {
        if let Node::Split { a, b, .. } = self {
            a.update(f);
            b.update(f);
        }

        f(self);
    }

    pub(crate) fn resize(&mut self, split: Split, percentage: f32) -> bool {
        match self {
            Node::Split {
                id, ratio, a, b, ..
            } => {
                if *id == split {
                    *ratio = percentage;

                    true
                } else if a.resize(split, percentage) {
                    true
                } else {
                    b.resize(split, percentage)
                }
            }
            Node::Pane(_) => false,
        }
    }

    pub(crate) fn resize_adjacent(
        &mut self,
        split: Split,
        percentage: f32,
        spacing: f32,
        panes: &BTreeMap<Pane, Constraints>,
        bounds: Size,
    ) -> bool {
        let Some((axis, region, _)) = self
            .split_regions_with_constraints(spacing, panes, bounds)
            .get(&split)
            .copied()
        else {
            return false;
        };
        let percentage = if percentage.is_finite() {
            percentage.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let desired_position =
            rectangle_origin(axis, &region) + rectangle_extent(axis, &region) * percentage;
        let carriers = self.resize_carriers(split, axis, spacing, panes, bounds);

        if !self.resize_adjacent_once(split, percentage, spacing, panes, bounds) {
            return false;
        }

        for (carrier, branch) in carriers {
            let Some((_, target_region, target_ratio)) = self
                .split_regions_with_constraints(spacing, panes, bounds)
                .get(&split)
                .copied()
            else {
                break;
            };
            let actual_position = split_position(axis, &target_region, target_ratio);
            let remaining = desired_position - actual_position;
            if remaining.abs() < 0.5 {
                break;
            }
            if (remaining > 0.0 && branch != Branch::A) || (remaining < 0.0 && branch != Branch::B)
            {
                continue;
            }

            let Some((carrier_axis, carrier_region, carrier_ratio)) = self
                .split_regions_with_constraints(spacing, panes, bounds)
                .get(&carrier)
                .copied()
            else {
                continue;
            };
            let carrier_position = split_position(carrier_axis, &carrier_region, carrier_ratio);
            let requested_ratio =
                ratio_at_position(carrier_axis, &carrier_region, carrier_position + remaining);
            let _ = self.resize_adjacent_once(carrier, requested_ratio, spacing, panes, bounds);

            let Some((target_axis, target_region, _)) = self
                .split_regions_with_constraints(spacing, panes, bounds)
                .get(&split)
                .copied()
            else {
                break;
            };
            let requested_ratio = ratio_at_position(target_axis, &target_region, desired_position);
            let _ = self.resize_adjacent_once(split, requested_ratio, spacing, panes, bounds);
        }

        true
    }

    pub(crate) fn resize_adjacent_at(
        &mut self,
        split: Split,
        position: f32,
        spacing: f32,
        panes: &BTreeMap<Pane, Constraints>,
        bounds: Size,
    ) -> bool {
        let Some((axis, visible_region, visible_ratio)) = self
            .split_regions_with_constraints(spacing, panes, bounds)
            .get(&split)
            .copied()
        else {
            return false;
        };
        let proxy = self.resize_proxy(split, axis, panes);
        let resize_panes = panes
            .iter()
            .map(|(pane, constraints)| (*pane, constraints.for_resize(axis)))
            .collect();
        let Some((proxy_axis, proxy_region, proxy_ratio)) = self
            .split_regions_with_constraints(spacing, &resize_panes, bounds)
            .get(&proxy)
            .copied()
        else {
            return false;
        };
        if axis != proxy_axis {
            return false;
        }

        let visible_position = split_position(axis, &visible_region, visible_ratio);
        let proxy_position = split_position(proxy_axis, &proxy_region, proxy_ratio);
        let ratio = ratio_at_position(
            proxy_axis,
            &proxy_region,
            proxy_position + position - visible_position,
        );
        self.resize_adjacent(proxy, ratio, spacing, &resize_panes, bounds)
    }

    fn resize_adjacent_once(
        &mut self,
        split: Split,
        percentage: f32,
        spacing: f32,
        panes: &BTreeMap<Pane, Constraints>,
        bounds: Size,
    ) -> bool {
        let constraints = self.constraints(panes, spacing);
        let bounds = Rectangle::new(crate::core::Point::ORIGIN, bounds);
        let mut snapshots = BTreeMap::new();
        self.compute_constrained_snapshots(spacing, &bounds, &constraints, &mut snapshots);
        self.resize_adjacent_inner(
            split,
            percentage,
            spacing,
            &bounds,
            &constraints,
            &snapshots,
        )
    }

    /// Splits that can absorb drag distance the resized `split` could not
    /// fulfil, ordered from the boundary outwards.
    ///
    /// A boundary only resizes the panes immediately next to it, so a drag
    /// that pins those panes at their minimum has to hand the leftover
    /// distance to the next boundary along the axis. That boundary is a
    /// descendant whenever the neighbouring pane is itself split — a rail
    /// stacked over a pane, say — and an ancestor once the resized subtree is
    /// exhausted. `Branch::A` marks a carrier that sits after the boundary and
    /// so absorbs forward drags; `Branch::B` marks one that sits before it.
    fn resize_carriers(
        &self,
        split: Split,
        axis: Axis,
        spacing: f32,
        panes: &BTreeMap<Pane, Constraints>,
        bounds: Size,
    ) -> Vec<(Split, Branch)> {
        let Some(Node::Split { a, b, .. }) = self.find_split(split) else {
            return self.same_axis_ancestors(split, axis);
        };

        let regions = self.split_regions_with_constraints(spacing, panes, bounds);
        let position = |split: &Split| {
            regions
                .get(split)
                .map_or(f32::INFINITY, |(axis, region, ratio)| {
                    split_position(*axis, region, *ratio)
                })
        };

        let mut after = b.axis_splits(axis);
        let mut before = a.axis_splits(axis);
        after.sort_by(|x, y| position(x).total_cmp(&position(y)));
        before.sort_by(|x, y| position(y).total_cmp(&position(x)));

        after
            .into_iter()
            .map(|split| (split, Branch::A))
            .chain(before.into_iter().map(|split| (split, Branch::B)))
            .chain(self.same_axis_ancestors(split, axis))
            .collect()
    }

    fn find_split(&self, split: Split) -> Option<&Node> {
        let Node::Split { id, a, b, .. } = self else {
            return None;
        };
        if *id == split {
            return Some(self);
        }

        a.find_split(split).or_else(|| b.find_split(split))
    }

    fn axis_splits(&self, axis: Axis) -> Vec<Split> {
        let mut splits = Vec::new();
        self.collect_axis_splits(axis, &mut splits);
        splits
    }

    fn collect_axis_splits(&self, axis: Axis, splits: &mut Vec<Split>) {
        let Node::Split {
            id,
            axis: split_axis,
            a,
            b,
            ..
        } = self
        else {
            return;
        };
        if *split_axis == axis {
            splits.push(*id);
        }
        a.collect_axis_splits(axis, splits);
        b.collect_axis_splits(axis, splits);
    }

    fn same_axis_ancestors(&self, split: Split, axis: Axis) -> Vec<(Split, Branch)> {
        self.ancestors(split)
            .into_iter()
            .filter(|(_, ancestor_axis, _)| *ancestor_axis == axis)
            .map(|(ancestor, _, branch)| (ancestor, branch))
            .collect()
    }

    fn ancestors(&self, split: Split) -> Vec<(Split, Axis, Branch)> {
        let mut path = Vec::new();
        if !self.split_path(split, &mut path) {
            return Vec::new();
        }
        path.into_iter().rev().collect()
    }

    fn resize_proxy(&self, split: Split, axis: Axis, panes: &BTreeMap<Pane, Constraints>) -> Split {
        let Some((a_fixed, b_fixed)) = self.split_fixed_sides(split, axis, panes) else {
            return split;
        };
        let fixed_branch = match (a_fixed, b_fixed) {
            (true, false) => Branch::A,
            (false, true) => Branch::B,
            _ => return split,
        };

        self.ancestors(split)
            .into_iter()
            .filter(|(_, ancestor_axis, _)| *ancestor_axis == axis)
            .map(|(ancestor, _, branch)| (ancestor, branch))
            .find_map(|(ancestor, branch)| {
                let crosses_fixed = matches!(
                    (fixed_branch, branch),
                    (Branch::A, Branch::B) | (Branch::B, Branch::A)
                );
                (crosses_fixed && self.opposite_has_resize_target(ancestor, branch, axis, panes))
                    .then_some(ancestor)
            })
            .unwrap_or(split)
    }

    fn split_fixed_sides(
        &self,
        split: Split,
        axis: Axis,
        panes: &BTreeMap<Pane, Constraints>,
    ) -> Option<(bool, bool)> {
        match self {
            Node::Split { id, a, b, .. } if *id == split => {
                Some((a.is_fixed(axis, panes), b.is_fixed(axis, panes)))
            }
            Node::Split { a, b, .. } => a
                .split_fixed_sides(split, axis, panes)
                .or_else(|| b.split_fixed_sides(split, axis, panes)),
            Node::Pane(_) => None,
        }
    }

    fn opposite_has_resize_target(
        &self,
        split: Split,
        branch: Branch,
        axis: Axis,
        panes: &BTreeMap<Pane, Constraints>,
    ) -> bool {
        match self {
            Node::Split { id, a, b, .. } if *id == split => match branch {
                Branch::A => !b.is_fixed(axis, panes),
                Branch::B => !a.is_fixed(axis, panes),
            },
            Node::Split { a, b, .. } => {
                a.opposite_has_resize_target(split, branch, axis, panes)
                    || b.opposite_has_resize_target(split, branch, axis, panes)
            }
            Node::Pane(_) => false,
        }
    }

    fn is_fixed(&self, axis: Axis, panes: &BTreeMap<Pane, Constraints>) -> bool {
        match self {
            Node::Pane(pane) => panes.get(pane).copied().unwrap_or_default().is_fixed(axis),
            Node::Split { a, b, .. } => a.is_fixed(axis, panes) && b.is_fixed(axis, panes),
        }
    }

    fn split_path(&self, split: Split, path: &mut Vec<(Split, Axis, Branch)>) -> bool {
        let Node::Split { id, axis, a, b, .. } = self else {
            return false;
        };
        if *id == split {
            return true;
        }

        path.push((*id, *axis, Branch::A));
        if a.split_path(split, path) {
            return true;
        }
        let _ = path.pop();

        path.push((*id, *axis, Branch::B));
        if b.split_path(split, path) {
            return true;
        }
        let _ = path.pop();
        false
    }

    fn resize_adjacent_inner(
        &mut self,
        split: Split,
        percentage: f32,
        spacing: f32,
        current: &Rectangle,
        constraints: &ConstraintTree,
        snapshots: &BTreeMap<Split, SplitSnapshot>,
    ) -> bool {
        let (
            Node::Split {
                id,
                axis,
                ratio,
                a,
                b,
            },
            ConstraintTree::Split {
                a: constraints_a,
                b: constraints_b,
                ..
            },
        ) = (self, constraints)
        else {
            return false;
        };

        let child_spacing = visible_spacing(
            *axis,
            spacing,
            constraints_a.constraints(),
            constraints_b.constraints(),
        );

        if *id == split {
            let a_constraints = boundary_extent_constraints(
                a,
                constraints_a,
                *axis,
                Boundary::End,
                spacing,
                snapshots,
            );
            let b_constraints = boundary_extent_constraints(
                b,
                constraints_b,
                *axis,
                Boundary::Start,
                spacing,
                snapshots,
            );
            let (region_a, region_b, actual_ratio) = split_with_extent_constraints(
                *axis,
                current,
                percentage,
                child_spacing,
                a_constraints,
                b_constraints,
            );
            *ratio = actual_ratio;
            a.preserve_boundary(
                *axis,
                Boundary::End,
                spacing,
                &region_a,
                constraints_a,
                snapshots,
            );
            b.preserve_boundary(
                *axis,
                Boundary::Start,
                spacing,
                &region_b,
                constraints_b,
                snapshots,
            );
            return true;
        }

        let (region_a, region_b, _) = split_constrained(
            *axis,
            current,
            *ratio,
            child_spacing,
            constraints_a.constraints(),
            constraints_b.constraints(),
        );
        a.resize_adjacent_inner(
            split,
            percentage,
            spacing,
            &region_a,
            constraints_a,
            snapshots,
        ) || b.resize_adjacent_inner(
            split,
            percentage,
            spacing,
            &region_b,
            constraints_b,
            snapshots,
        )
    }

    fn preserve_boundary(
        &mut self,
        resized_axis: Axis,
        boundary: Boundary,
        spacing: f32,
        current: &Rectangle,
        constraints: &ConstraintTree,
        snapshots: &BTreeMap<Split, SplitSnapshot>,
    ) {
        let (
            Node::Split {
                id,
                axis,
                ratio,
                a,
                b,
            },
            ConstraintTree::Split {
                a: constraints_a,
                b: constraints_b,
                ..
            },
        ) = (self, constraints)
        else {
            return;
        };

        let child_spacing = visible_spacing(
            *axis,
            spacing,
            constraints_a.constraints(),
            constraints_b.constraints(),
        );
        if *axis == resized_axis {
            match boundary {
                Boundary::Start if constraints_a.constraints().is_hidden(*axis) => {
                    let (_, region_b, _) = split_constrained(
                        *axis,
                        current,
                        *ratio,
                        child_spacing,
                        constraints_a.constraints(),
                        constraints_b.constraints(),
                    );
                    b.preserve_boundary(
                        resized_axis,
                        boundary,
                        spacing,
                        &region_b,
                        constraints_b,
                        snapshots,
                    );
                    return;
                }
                Boundary::End if constraints_b.constraints().is_hidden(*axis) => {
                    let (region_a, _, _) = split_constrained(
                        *axis,
                        current,
                        *ratio,
                        child_spacing,
                        constraints_a.constraints(),
                        constraints_b.constraints(),
                    );
                    a.preserve_boundary(
                        resized_axis,
                        boundary,
                        spacing,
                        &region_a,
                        constraints_a,
                        snapshots,
                    );
                    return;
                }
                Boundary::Start | Boundary::End => {}
            }
            let Some(snapshot) = snapshots.get(id) else {
                return;
            };
            let available = rectangle_extent(resized_axis, current) - child_spacing;
            let size_a = match boundary {
                Boundary::Start => available - rectangle_extent(resized_axis, &snapshot.b),
                Boundary::End => rectangle_extent(resized_axis, &snapshot.a),
            };
            let requested_ratio = ratio_for_extent(resized_axis, current, child_spacing, size_a);
            let (region_a, region_b, actual_ratio) = split_constrained(
                *axis,
                current,
                requested_ratio,
                child_spacing,
                constraints_a.constraints(),
                constraints_b.constraints(),
            );
            *ratio = actual_ratio;
            match boundary {
                Boundary::Start => a.preserve_boundary(
                    resized_axis,
                    boundary,
                    spacing,
                    &region_a,
                    constraints_a,
                    snapshots,
                ),
                Boundary::End => b.preserve_boundary(
                    resized_axis,
                    boundary,
                    spacing,
                    &region_b,
                    constraints_b,
                    snapshots,
                ),
            }
            return;
        }

        let (region_a, region_b, _) = split_constrained(
            *axis,
            current,
            *ratio,
            child_spacing,
            constraints_a.constraints(),
            constraints_b.constraints(),
        );
        a.preserve_boundary(
            resized_axis,
            boundary,
            spacing,
            &region_a,
            constraints_a,
            snapshots,
        );
        b.preserve_boundary(
            resized_axis,
            boundary,
            spacing,
            &region_b,
            constraints_b,
            snapshots,
        );
    }

    pub(crate) fn remove(&mut self, pane: Pane) -> Option<Pane> {
        match self {
            Node::Split { a, b, .. } => {
                if a.pane() == Some(pane) {
                    *self = *b.clone();
                    Some(self.first_pane())
                } else if b.pane() == Some(pane) {
                    *self = *a.clone();
                    Some(self.first_pane())
                } else {
                    a.remove(pane).or_else(|| b.remove(pane))
                }
            }
            Node::Pane(_) => None,
        }
    }

    fn pane(&self) -> Option<Pane> {
        match self {
            Node::Split { .. } => None,
            Node::Pane(pane) => Some(*pane),
        }
    }

    fn first_pane(&self) -> Pane {
        match self {
            Node::Split { a, .. } => a.first_pane(),
            Node::Pane(pane) => *pane,
        }
    }

    fn compute_regions(
        &self,
        spacing: f32,
        min_size: f32,
        current: &Rectangle,
        count: &Count,
        regions: &mut BTreeMap<Pane, Rectangle>,
    ) {
        match (self, count) {
            (
                Node::Split {
                    axis, ratio, a, b, ..
                },
                Count::Split {
                    a: count_a,
                    b: count_b,
                    ..
                },
            ) => {
                let (a_factor, b_factor) = match axis {
                    Axis::Horizontal => (count_a.horizontal(), count_b.horizontal()),
                    Axis::Vertical => (count_a.vertical(), count_b.vertical()),
                };

                let (region_a, region_b, _ratio) = axis.split(
                    current,
                    *ratio,
                    spacing,
                    min_size * (a_factor + 1) as f32 + spacing * a_factor as f32,
                    min_size * (b_factor + 1) as f32 + spacing * b_factor as f32,
                );

                a.compute_regions(spacing, min_size, &region_a, count_a, regions);
                b.compute_regions(spacing, min_size, &region_b, count_b, regions);
            }
            (Node::Pane(pane), Count::Pane) => {
                let _ = regions.insert(*pane, *current);
            }
            _ => {
                unreachable!("Node configuration and count do not match")
            }
        }
    }

    fn compute_splits(
        &self,
        spacing: f32,
        min_size: f32,
        current: &Rectangle,
        count: &Count,
        splits: &mut BTreeMap<Split, (Axis, Rectangle, f32)>,
    ) {
        match (self, count) {
            (
                Node::Split {
                    axis,
                    ratio,
                    a,
                    b,
                    id,
                },
                Count::Split {
                    a: count_a,
                    b: count_b,
                    ..
                },
            ) => {
                let (a_factor, b_factor) = match axis {
                    Axis::Horizontal => (count_a.horizontal(), count_b.horizontal()),
                    Axis::Vertical => (count_a.vertical(), count_b.vertical()),
                };

                let (region_a, region_b, ratio) = axis.split(
                    current,
                    *ratio,
                    spacing,
                    min_size * (a_factor + 1) as f32 + spacing * a_factor as f32,
                    min_size * (b_factor + 1) as f32 + spacing * b_factor as f32,
                );

                let _ = splits.insert(*id, (*axis, *current, ratio));

                a.compute_splits(spacing, min_size, &region_a, count_a, splits);
                b.compute_splits(spacing, min_size, &region_b, count_b, splits);
            }
            (Node::Pane(_), Count::Pane) => {}
            _ => {
                unreachable!("Node configuration and split count do not match")
            }
        }
    }

    fn compute_constrained_regions(
        &self,
        spacing: f32,
        current: &Rectangle,
        constraints: &ConstraintTree,
        regions: &mut BTreeMap<Pane, Rectangle>,
    ) {
        match (self, constraints) {
            (
                Node::Split {
                    axis, ratio, a, b, ..
                },
                ConstraintTree::Split {
                    a: constraints_a,
                    b: constraints_b,
                    ..
                },
            ) => {
                let (region_a, region_b, _) = split_constrained(
                    *axis,
                    current,
                    *ratio,
                    visible_spacing(
                        *axis,
                        spacing,
                        constraints_a.constraints(),
                        constraints_b.constraints(),
                    ),
                    constraints_a.constraints(),
                    constraints_b.constraints(),
                );

                a.compute_constrained_regions(spacing, &region_a, constraints_a, regions);
                b.compute_constrained_regions(spacing, &region_b, constraints_b, regions);
            }
            (Node::Pane(pane), ConstraintTree::Pane(_)) => {
                let _ = regions.insert(*pane, *current);
            }
            _ => unreachable!("Node configuration and constraints do not match"),
        }
    }

    fn compute_constrained_splits(
        &self,
        spacing: f32,
        current: &Rectangle,
        constraints: &ConstraintTree,
        splits: &mut BTreeMap<Split, (Axis, Rectangle, f32)>,
    ) {
        match (self, constraints) {
            (
                Node::Split {
                    id,
                    axis,
                    ratio,
                    a,
                    b,
                },
                ConstraintTree::Split {
                    a: constraints_a,
                    b: constraints_b,
                    ..
                },
            ) => {
                let child_spacing = visible_spacing(
                    *axis,
                    spacing,
                    constraints_a.constraints(),
                    constraints_b.constraints(),
                );
                let (region_a, region_b, ratio) = split_constrained(
                    *axis,
                    current,
                    *ratio,
                    child_spacing,
                    constraints_a.constraints(),
                    constraints_b.constraints(),
                );
                if !constraints_a.constraints().is_hidden(*axis)
                    && !constraints_b.constraints().is_hidden(*axis)
                {
                    let _ = splits.insert(*id, (*axis, *current, ratio));
                }

                a.compute_constrained_splits(spacing, &region_a, constraints_a, splits);
                b.compute_constrained_splits(spacing, &region_b, constraints_b, splits);
            }
            (Node::Pane(_), ConstraintTree::Pane(_)) => {}
            _ => unreachable!("Node configuration and constraints do not match"),
        }
    }

    fn compute_constrained_snapshots(
        &self,
        spacing: f32,
        current: &Rectangle,
        constraints: &ConstraintTree,
        snapshots: &mut BTreeMap<Split, SplitSnapshot>,
    ) {
        let (
            Node::Split {
                id,
                axis,
                ratio,
                a,
                b,
            },
            ConstraintTree::Split {
                a: constraints_a,
                b: constraints_b,
                ..
            },
        ) = (self, constraints)
        else {
            return;
        };

        let child_spacing = visible_spacing(
            *axis,
            spacing,
            constraints_a.constraints(),
            constraints_b.constraints(),
        );
        let (region_a, region_b, _) = split_constrained(
            *axis,
            current,
            *ratio,
            child_spacing,
            constraints_a.constraints(),
            constraints_b.constraints(),
        );
        let _ = snapshots.insert(
            *id,
            SplitSnapshot {
                a: region_a,
                b: region_b,
            },
        );
        a.compute_constrained_snapshots(spacing, &region_a, constraints_a, snapshots);
        b.compute_constrained_snapshots(spacing, &region_b, constraints_b, snapshots);
    }
}

fn boundary_extent_constraints(
    node: &Node,
    constraints: &ConstraintTree,
    resized_axis: Axis,
    boundary: Boundary,
    spacing: f32,
    snapshots: &BTreeMap<Split, SplitSnapshot>,
) -> ExtentConstraints {
    let (
        Node::Split { id, axis, a, b, .. },
        ConstraintTree::Split {
            a: constraints_a,
            b: constraints_b,
            ..
        },
    ) = (node, constraints)
    else {
        return constraint_extent(resized_axis, constraints.constraints());
    };

    let child_spacing = visible_spacing(
        *axis,
        spacing,
        constraints_a.constraints(),
        constraints_b.constraints(),
    );

    if *axis == resized_axis {
        match boundary {
            Boundary::Start if constraints_a.constraints().is_hidden(*axis) => {
                return boundary_extent_constraints(
                    b,
                    constraints_b,
                    resized_axis,
                    boundary,
                    spacing,
                    snapshots,
                );
            }
            Boundary::End if constraints_b.constraints().is_hidden(*axis) => {
                return boundary_extent_constraints(
                    a,
                    constraints_a,
                    resized_axis,
                    boundary,
                    spacing,
                    snapshots,
                );
            }
            Boundary::Start | Boundary::End => {}
        }
        let Some(snapshot) = snapshots.get(id) else {
            return constraint_extent(resized_axis, constraints.constraints());
        };
        return match boundary {
            Boundary::Start => boundary_extent_constraints(
                a,
                constraints_a,
                resized_axis,
                boundary,
                spacing,
                snapshots,
            )
            .plus(rectangle_extent(resized_axis, &snapshot.b) + child_spacing),
            Boundary::End => boundary_extent_constraints(
                b,
                constraints_b,
                resized_axis,
                boundary,
                spacing,
                snapshots,
            )
            .plus(rectangle_extent(resized_axis, &snapshot.a) + child_spacing),
        };
    }

    let a =
        boundary_extent_constraints(a, constraints_a, resized_axis, boundary, spacing, snapshots);
    let b =
        boundary_extent_constraints(b, constraints_b, resized_axis, boundary, spacing, snapshots);
    ExtentConstraints {
        min: a.min.max(b.min),
        max: a.max.min(b.max),
    }
    .normalized()
}

impl ExtentConstraints {
    fn plus(self, extent: f32) -> Self {
        Self {
            min: self.min + extent,
            max: self.max + extent,
        }
    }

    fn normalized(self) -> Self {
        Self {
            min: self.min.max(0.0),
            max: self.max.max(self.min).max(0.0),
        }
    }
}

fn constraint_extent(axis: Axis, constraints: Constraints) -> ExtentConstraints {
    match axis {
        Axis::Horizontal => ExtentConstraints {
            min: constraints.min.height,
            max: constraints.max.height,
        },
        Axis::Vertical => ExtentConstraints {
            min: constraints.min.width,
            max: constraints.max.width,
        },
    }
}

fn rectangle_extent(axis: Axis, rectangle: &Rectangle) -> f32 {
    match axis {
        Axis::Horizontal => rectangle.height,
        Axis::Vertical => rectangle.width,
    }
}

fn rectangle_origin(axis: Axis, rectangle: &Rectangle) -> f32 {
    match axis {
        Axis::Horizontal => rectangle.y,
        Axis::Vertical => rectangle.x,
    }
}

fn split_position(axis: Axis, rectangle: &Rectangle, ratio: f32) -> f32 {
    rectangle_origin(axis, rectangle) + rectangle_extent(axis, rectangle) * ratio
}

fn ratio_at_position(axis: Axis, rectangle: &Rectangle, position: f32) -> f32 {
    let extent = rectangle_extent(axis, rectangle);
    if extent > 0.0 {
        ((position - rectangle_origin(axis, rectangle)) / extent).clamp(0.0, 1.0)
    } else {
        0.5
    }
}

fn ratio_for_extent(axis: Axis, current: &Rectangle, spacing: f32, size_a: f32) -> f32 {
    let extent = rectangle_extent(axis, current);
    if extent > 0.0 {
        (size_a + spacing / 2.0) / extent
    } else {
        0.5
    }
}

fn split_with_extent_constraints(
    axis: Axis,
    current: &Rectangle,
    ratio: f32,
    spacing: f32,
    a: ExtentConstraints,
    b: ExtentConstraints,
) -> (Rectangle, Rectangle, f32) {
    axis.split_with_constraints(current, ratio, spacing, a.min, a.max, b.min, b.max)
}

fn visible_spacing(axis: Axis, spacing: f32, a: Constraints, b: Constraints) -> f32 {
    if a.is_hidden(axis) || b.is_hidden(axis) {
        0.0
    } else {
        spacing
    }
}

fn split_constrained(
    axis: Axis,
    current: &Rectangle,
    ratio: f32,
    spacing: f32,
    a: Constraints,
    b: Constraints,
) -> (Rectangle, Rectangle, f32) {
    match axis {
        Axis::Horizontal => axis.split_with_constraints(
            current,
            ratio,
            spacing,
            a.min.height,
            a.max.height,
            b.min.height,
            b.max.height,
        ),
        Axis::Vertical => axis.split_with_constraints(
            current,
            ratio,
            spacing,
            a.min.width,
            a.max.width,
            b.min.width,
            b.max.width,
        ),
    }
}

impl std::hash::Hash for Node {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Node::Split {
                id,
                axis,
                ratio,
                a,
                b,
            } => {
                id.hash(state);
                axis.hash(state);
                ((ratio * 100_000.0) as u32).hash(state);
                a.hash(state);
                b.hash(state);
            }
            Node::Pane(pane) => {
                pane.hash(state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INNER: Split = Split(0);
    const OUTER: Split = Split(1);

    fn constraints() -> BTreeMap<Pane, Constraints> {
        constraints_for(3)
    }

    fn constraints_for(count: usize) -> BTreeMap<Pane, Constraints> {
        (0..count)
            .map(|id| (Pane(id), Constraints::minimum(Size::new(100.0, 0.0))))
            .collect()
    }

    fn widths(node: &Node) -> Vec<f32> {
        let regions =
            node.pane_regions_with_constraints(0.0, &constraints(), Size::new(900.0, 600.0));
        (0..3).map(|id| regions[&Pane(id)].width).collect()
    }

    #[test]
    fn resize_adjacent_preserves_non_adjacent_panes() {
        let mut left_nested = Node::Split {
            id: OUTER,
            axis: Axis::Vertical,
            ratio: 2.0 / 3.0,
            a: Box::new(Node::Split {
                id: INNER,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Pane(Pane(0))),
                b: Box::new(Node::Pane(Pane(1))),
            }),
            b: Box::new(Node::Pane(Pane(2))),
        };

        assert!(left_nested.resize_adjacent(
            OUTER,
            0.5,
            0.0,
            &constraints(),
            Size::new(900.0, 600.0)
        ));
        assert_eq!(vec![300.0, 150.0, 450.0], widths(&left_nested));

        let mut right_nested = Node::Split {
            id: OUTER,
            axis: Axis::Vertical,
            ratio: 1.0 / 3.0,
            a: Box::new(Node::Pane(Pane(0))),
            b: Box::new(Node::Split {
                id: INNER,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Pane(Pane(1))),
                b: Box::new(Node::Pane(Pane(2))),
            }),
        };

        assert!(right_nested.resize_adjacent(
            OUTER,
            0.5,
            0.0,
            &constraints(),
            Size::new(900.0, 600.0)
        ));
        assert_eq!(vec![450.0, 150.0, 300.0], widths(&right_nested));
    }

    #[test]
    fn resize_adjacent_cascades_past_the_adjacent_minimum() {
        let mut node = Node::Split {
            id: OUTER,
            axis: Axis::Vertical,
            ratio: 2.0 / 3.0,
            a: Box::new(Node::Split {
                id: INNER,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Pane(Pane(0))),
                b: Box::new(Node::Pane(Pane(1))),
            }),
            b: Box::new(Node::Pane(Pane(2))),
        };

        assert!(node.resize_adjacent(OUTER, 0.2, 0.0, &constraints(), Size::new(900.0, 600.0)));
        assert_eq!(vec![100.0, 100.0, 700.0], widths(&node));
    }

    #[test]
    fn resize_adjacent_pushes_the_next_pane_after_the_minimum() {
        let mut node = Node::Split {
            id: OUTER,
            axis: Axis::Vertical,
            ratio: 2.0 / 3.0,
            a: Box::new(Node::Split {
                id: INNER,
                axis: Axis::Vertical,
                ratio: 0.5,
                a: Box::new(Node::Pane(Pane(0))),
                b: Box::new(Node::Pane(Pane(1))),
            }),
            b: Box::new(Node::Pane(Pane(2))),
        };

        assert!(node.resize_adjacent(INNER, 0.9, 0.0, &constraints(), Size::new(900.0, 600.0)));
        assert_eq!(vec![540.0, 100.0, 260.0], widths(&node));
    }

    #[test]
    fn resize_adjacent_pushes_past_perpendicular_ancestors_after_the_minimum() {
        for (name, axis, size) in [
            ("Width", Axis::Vertical, Size::new(900.0, 600.0)),
            ("Height", Axis::Horizontal, Size::new(900.0, 600.0)),
        ] {
            let perpendicular = match axis {
                Axis::Horizontal => Axis::Vertical,
                Axis::Vertical => Axis::Horizontal,
            };
            let mut node = Node::Split {
                id: OUTER,
                axis,
                ratio: 1.0 / 3.0,
                a: Box::new(Node::Pane(Pane(0))),
                b: Box::new(Node::Split {
                    id: Split(2),
                    axis: perpendicular,
                    ratio: 0.5,
                    a: Box::new(Node::Split {
                        id: INNER,
                        axis,
                        ratio: 0.5,
                        a: Box::new(Node::Pane(Pane(1))),
                        b: Box::new(Node::Pane(Pane(2))),
                    }),
                    b: Box::new(Node::Pane(Pane(3))),
                }),
            };
            let minimum = match axis {
                Axis::Horizontal => Constraints::minimum(Size::new(0.0, 100.0)),
                Axis::Vertical => Constraints::minimum(Size::new(100.0, 0.0)),
            };
            let constraints = (0..4).map(|id| (Pane(id), minimum)).collect();
            let before = node.pane_regions_with_constraints(0.0, &constraints, size);
            let boundary = rectangle_end(axis, &before[&Pane(1)]);

            assert!(node.resize_adjacent_at(INNER, boundary - 300.0, 0.0, &constraints, size));

            let after = node.pane_regions_with_constraints(0.0, &constraints, size);
            assert_close(100.0, rectangle_extent(axis, &after[&Pane(1)]), name);
            assert_close(
                rectangle_extent(axis, &before[&Pane(0)]) - 100.0,
                rectangle_extent(axis, &after[&Pane(0)]),
                name,
            );
        }
    }

    #[test]
    fn resize_adjacent_skips_hidden_boundary_panes() {
        let root = Split(2);
        let mut node = Node::Split {
            id: root,
            axis: Axis::Vertical,
            ratio: 2.0 / 3.0,
            a: Box::new(Node::Split {
                id: OUTER,
                axis: Axis::Vertical,
                ratio: 1.0,
                a: Box::new(Node::Split {
                    id: INNER,
                    axis: Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Node::Pane(Pane(0))),
                    b: Box::new(Node::Pane(Pane(1))),
                }),
                b: Box::new(Node::Pane(Pane(2))),
            }),
            b: Box::new(Node::Pane(Pane(3))),
        };
        let mut constraints = constraints_for(4);
        let _ = constraints.insert(Pane(2), Constraints::fixed(Size::ZERO));

        assert!(node.resize_adjacent(root, 0.5, 0.0, &constraints, Size::new(900.0, 600.0)));
        let regions =
            node.pane_regions_with_constraints(0.0, &constraints, Size::new(900.0, 600.0));
        assert_eq!(300.0, regions[&Pane(0)].width);
        assert_eq!(150.0, regions[&Pane(1)].width);
        assert_eq!(0.0, regions[&Pane(2)].width);
        assert_eq!(450.0, regions[&Pane(3)].width);
    }

    #[test]
    fn resize_adjacent_passes_through_fixed_rail_stacks() {
        for (name, axis, size) in [
            ("Width", Axis::Vertical, Size::new(900.0, 600.0)),
            ("Height", Axis::Horizontal, Size::new(900.0, 600.0)),
        ] {
            let perpendicular = match axis {
                Axis::Horizontal => Axis::Vertical,
                Axis::Vertical => Axis::Horizontal,
            };
            let root = Split(2);
            let node = Node::Split {
                id: root,
                axis,
                ratio: 2.0 / 3.0,
                a: Box::new(Node::Split {
                    id: INNER,
                    axis,
                    ratio: 0.95,
                    a: Box::new(Node::Pane(Pane(0))),
                    b: Box::new(Node::Split {
                        id: OUTER,
                        axis: perpendicular,
                        ratio: 0.5,
                        a: Box::new(Node::Pane(Pane(1))),
                        b: Box::new(Node::Pane(Pane(2))),
                    }),
                }),
                b: Box::new(Node::Pane(Pane(3))),
            };
            let expanded = match axis {
                Axis::Horizontal => Constraints::minimum(Size::new(0.0, 100.0)),
                Axis::Vertical => Constraints::minimum(Size::new(100.0, 0.0)),
            };
            let rail = match axis {
                Axis::Horizontal => {
                    Constraints::new(Size::new(0.0, 30.0), Size::new(f32::INFINITY, 30.0))
                }
                Axis::Vertical => {
                    Constraints::new(Size::new(30.0, 0.0), Size::new(30.0, f32::INFINITY))
                }
            };
            let constraints: BTreeMap<Pane, Constraints> = [
                (Pane(0), expanded),
                (Pane(1), rail),
                (Pane(2), rail),
                (Pane(3), expanded),
            ]
            .into_iter()
            .collect();

            let mut flexible_node = node.clone();
            let mut flexible_constraints = constraints.clone();
            let _ = flexible_constraints.insert(Pane(1), expanded);
            let before =
                flexible_node.pane_regions_with_constraints(0.0, &flexible_constraints, size);
            let boundary = rectangle_end(axis, &before[&Pane(0)]);

            assert!(flexible_node.resize_adjacent_at(
                INNER,
                boundary - 100.0,
                0.0,
                &flexible_constraints,
                size
            ));

            let after =
                flexible_node.pane_regions_with_constraints(0.0, &flexible_constraints, size);
            assert_close(
                rectangle_extent(axis, &before[&Pane(3)]),
                rectangle_extent(axis, &after[&Pane(3)]),
                name,
            );

            let mut node = node;
            let before = node.pane_regions_with_constraints(0.0, &constraints, size);
            let boundary = rectangle_end(axis, &before[&Pane(0)]);

            assert!(node.resize_adjacent_at(INNER, boundary - 100.0, 0.0, &constraints, size));

            let after = node.pane_regions_with_constraints(0.0, &constraints, size);
            assert_close(
                rectangle_extent(axis, &before[&Pane(0)]) - 100.0,
                rectangle_extent(axis, &after[&Pane(0)]),
                name,
            );
            assert_close(
                rectangle_extent(axis, &before[&Pane(3)]) + 100.0,
                rectangle_extent(axis, &after[&Pane(3)]),
                name,
            );
            assert_close(30.0, rectangle_extent(axis, &after[&Pane(1)]), name);
            assert_close(30.0, rectangle_extent(axis, &after[&Pane(2)]), name);
        }
    }

    #[test]
    fn resize_adjacent_passes_through_perpendicular_ancestors() {
        for (name, axis, size) in [
            ("Width", Axis::Vertical, Size::new(900.0, 600.0)),
            ("Height", Axis::Horizontal, Size::new(900.0, 600.0)),
        ] {
            let perpendicular = match axis {
                Axis::Horizontal => Axis::Vertical,
                Axis::Vertical => Axis::Horizontal,
            };
            let mut node = Node::Split {
                id: Split(3),
                axis,
                ratio: 2.0 / 3.0,
                a: Box::new(Node::Split {
                    id: Split(2),
                    axis: perpendicular,
                    ratio: 0.7,
                    a: Box::new(Node::Split {
                        id: INNER,
                        axis,
                        ratio: 0.95,
                        a: Box::new(Node::Pane(Pane(0))),
                        b: Box::new(Node::Pane(Pane(1))),
                    }),
                    b: Box::new(Node::Pane(Pane(2))),
                }),
                b: Box::new(Node::Pane(Pane(3))),
            };
            let expanded = match axis {
                Axis::Horizontal => Constraints::minimum(Size::new(0.0, 100.0)),
                Axis::Vertical => Constraints::minimum(Size::new(100.0, 0.0)),
            };
            let rail = match axis {
                Axis::Horizontal => {
                    Constraints::new(Size::new(0.0, 30.0), Size::new(f32::INFINITY, 30.0))
                }
                Axis::Vertical => {
                    Constraints::new(Size::new(30.0, 0.0), Size::new(30.0, f32::INFINITY))
                }
            };
            let perpendicular_rail = match perpendicular {
                Axis::Horizontal => {
                    Constraints::new(Size::new(0.0, 30.0), Size::new(f32::INFINITY, 30.0))
                }
                Axis::Vertical => {
                    Constraints::new(Size::new(30.0, 0.0), Size::new(30.0, f32::INFINITY))
                }
            };
            let constraints = [
                (Pane(0), expanded),
                (Pane(1), rail),
                (Pane(2), perpendicular_rail),
                (Pane(3), expanded),
            ]
            .into_iter()
            .collect();
            let before = node.pane_regions_with_constraints(0.0, &constraints, size);
            let boundary = rectangle_end(axis, &before[&Pane(0)]);

            assert!(node.resize_adjacent_at(INNER, boundary - 100.0, 0.0, &constraints, size));

            let after = node.pane_regions_with_constraints(0.0, &constraints, size);
            assert_close(
                rectangle_extent(axis, &before[&Pane(0)]) - 100.0,
                rectangle_extent(axis, &after[&Pane(0)]),
                name,
            );
            assert_close(
                rectangle_extent(axis, &before[&Pane(3)]) + 100.0,
                rectangle_extent(axis, &after[&Pane(3)]),
                name,
            );
            assert_close(30.0, rectangle_extent(axis, &after[&Pane(1)]), name);
            assert_close(
                30.0,
                rectangle_extent(perpendicular, &after[&Pane(2)]),
                name,
            );
        }
    }

    #[test]
    fn resize_adjacent_moves_both_edges_of_a_near_side_fixed_rail() {
        for (name, axis, size) in [
            ("Width", Axis::Vertical, Size::new(900.0, 600.0)),
            ("Height", Axis::Horizontal, Size::new(900.0, 600.0)),
        ] {
            let perpendicular = match axis {
                Axis::Horizontal => Axis::Vertical,
                Axis::Vertical => Axis::Horizontal,
            };
            let node = Node::Split {
                id: OUTER,
                axis,
                ratio: 1.0 / 3.0,
                a: Box::new(Node::Pane(Pane(0))),
                b: Box::new(Node::Split {
                    id: Split(2),
                    axis: perpendicular,
                    ratio: 0.95,
                    a: Box::new(Node::Split {
                        id: INNER,
                        axis,
                        ratio: 0.05,
                        a: Box::new(Node::Pane(Pane(1))),
                        b: Box::new(Node::Pane(Pane(2))),
                    }),
                    b: Box::new(Node::Pane(Pane(3))),
                }),
            };
            let expanded = match axis {
                Axis::Horizontal => Constraints::minimum(Size::new(0.0, 100.0)),
                Axis::Vertical => Constraints::minimum(Size::new(100.0, 0.0)),
            };
            let rail = match axis {
                Axis::Horizontal => {
                    Constraints::new(Size::new(0.0, 30.0), Size::new(f32::INFINITY, 30.0))
                }
                Axis::Vertical => {
                    Constraints::new(Size::new(30.0, 0.0), Size::new(30.0, f32::INFINITY))
                }
            };
            let perpendicular_rail = match perpendicular {
                Axis::Horizontal => {
                    Constraints::new(Size::new(0.0, 30.0), Size::new(f32::INFINITY, 30.0))
                }
                Axis::Vertical => {
                    Constraints::new(Size::new(30.0, 0.0), Size::new(30.0, f32::INFINITY))
                }
            };
            let constraints = [
                (Pane(0), expanded),
                (Pane(1), rail),
                (Pane(2), expanded),
                (Pane(3), perpendicular_rail),
            ]
            .into_iter()
            .collect();
            let before = node.pane_regions_with_constraints(0.0, &constraints, size);
            let outer_edge = rectangle_end(axis, &before[&Pane(0)]);
            let inner_edge = rectangle_end(axis, &before[&Pane(1)]);

            for (edge, split, position) in [
                ("Outer", OUTER, outer_edge + 100.0),
                ("Inner", INNER, inner_edge + 100.0),
            ] {
                let mut resized = node.clone();
                assert!(resized.resize_adjacent_at(split, position, 0.0, &constraints, size));

                let after = resized.pane_regions_with_constraints(0.0, &constraints, size);
                assert_close(
                    rectangle_extent(axis, &before[&Pane(0)]) + 100.0,
                    rectangle_extent(axis, &after[&Pane(0)]),
                    edge,
                );
                assert_close(30.0, rectangle_extent(axis, &after[&Pane(1)]), name);
            }
        }
    }

    #[test]
    fn resize_adjacent_stops_at_the_first_flexible_pane() {
        let size = Size::new(1000.0, 600.0);
        let mut node = Node::Split {
            id: Split(2),
            axis: Axis::Vertical,
            ratio: 0.75,
            a: Box::new(Node::Split {
                id: OUTER,
                axis: Axis::Vertical,
                ratio: 350.0 / 750.0,
                a: Box::new(Node::Split {
                    id: INNER,
                    axis: Axis::Vertical,
                    ratio: 250.0 / 350.0,
                    a: Box::new(Node::Pane(Pane(0))),
                    b: Box::new(Node::Pane(Pane(1))),
                }),
                b: Box::new(Node::Pane(Pane(2))),
            }),
            b: Box::new(Node::Pane(Pane(3))),
        };
        let constraints = constraints_for(4);
        let before = node.pane_regions_with_constraints(0.0, &constraints, size);
        assert_eq!(
            vec![250.0, 100.0, 400.0, 250.0],
            (0..4).map(|id| before[&Pane(id)].width).collect::<Vec<_>>()
        );

        let boundary = rectangle_end(Axis::Vertical, &before[&Pane(0)]);
        assert!(node.resize_adjacent_at(INNER, boundary + 100.0, 0.0, &constraints, size));

        let after = node.pane_regions_with_constraints(0.0, &constraints, size);
        for (id, expected) in [(0, 350.0), (1, 100.0), (2, 300.0), (3, 250.0)] {
            assert_close(expected, after[&Pane(id)].width, &format!("Pane {id}"));
        }
    }

    #[test]
    fn resize_adjacent_stops_at_a_flexible_perpendicular_subtree() {
        let size = Size::new(1000.0, 600.0);
        let mut node = Node::Split {
            id: Split(3),
            axis: Axis::Vertical,
            ratio: 0.75,
            a: Box::new(Node::Split {
                id: OUTER,
                axis: Axis::Vertical,
                ratio: 350.0 / 750.0,
                a: Box::new(Node::Split {
                    id: INNER,
                    axis: Axis::Vertical,
                    ratio: 250.0 / 350.0,
                    a: Box::new(Node::Pane(Pane(0))),
                    b: Box::new(Node::Pane(Pane(1))),
                }),
                b: Box::new(Node::Split {
                    id: Split(2),
                    axis: Axis::Horizontal,
                    ratio: 0.7,
                    a: Box::new(Node::Pane(Pane(2))),
                    b: Box::new(Node::Pane(Pane(4))),
                }),
            }),
            b: Box::new(Node::Pane(Pane(3))),
        };
        let constraints: BTreeMap<Pane, Constraints> = [0, 1, 2, 3, 4]
            .into_iter()
            .map(|id| (Pane(id), Constraints::minimum(Size::new(100.0, 0.0))))
            .collect();
        let before = node.pane_regions_with_constraints(0.0, &constraints, size);
        assert_eq!(
            vec![250.0, 100.0, 400.0, 250.0, 400.0],
            (0..5).map(|id| before[&Pane(id)].width).collect::<Vec<_>>()
        );

        let boundary = rectangle_end(Axis::Vertical, &before[&Pane(0)]);
        assert!(node.resize_adjacent_at(INNER, boundary + 100.0, 0.0, &constraints, size));

        let after = node.pane_regions_with_constraints(0.0, &constraints, size);
        for (id, expected) in [(0, 350.0), (1, 100.0), (2, 300.0), (3, 250.0), (4, 300.0)] {
            assert_close(expected, after[&Pane(id)].width, &format!("Pane {id}"));
        }
    }

    #[test]
    fn resize_adjacent_pushes_the_next_pane_inside_a_perpendicular_subtree() {
        const AGENT: Pane = Pane(0);
        const FILES: Pane = Pane(1);
        const DIFF: Pane = Pane(2);
        const TERMINAL: Pane = Pane(3);
        const PREVIEW: Pane = Pane(4);

        let size = Size::new(1350.0, 900.0);
        let mut node = Node::Split {
            id: Split(3),
            axis: Axis::Vertical,
            ratio: 950.0 / 1350.0,
            a: Box::new(Node::Split {
                id: OUTER,
                axis: Axis::Vertical,
                ratio: 300.0 / 950.0,
                a: Box::new(Node::Pane(AGENT)),
                b: Box::new(Node::Split {
                    id: Split(2),
                    axis: Axis::Horizontal,
                    ratio: 0.7,
                    a: Box::new(Node::Split {
                        id: INNER,
                        axis: Axis::Vertical,
                        ratio: 300.0 / 650.0,
                        a: Box::new(Node::Pane(FILES)),
                        b: Box::new(Node::Pane(DIFF)),
                    }),
                    b: Box::new(Node::Pane(TERMINAL)),
                }),
            }),
            b: Box::new(Node::Pane(PREVIEW)),
        };
        let constraints: BTreeMap<Pane, Constraints> = (0..5)
            .map(|id| (Pane(id), Constraints::minimum(Size::new(300.0, 0.0))))
            .collect();
        let before = node.pane_regions_with_constraints(0.0, &constraints, size);
        assert_eq!(
            vec![300.0, 300.0, 350.0, 650.0, 400.0],
            (0..5).map(|id| before[&Pane(id)].width).collect::<Vec<_>>()
        );

        let boundary = rectangle_end(Axis::Vertical, &before[&AGENT]);
        assert!(node.resize_adjacent_at(OUTER, boundary + 30.0, 0.0, &constraints, size));

        let after = node.pane_regions_with_constraints(0.0, &constraints, size);
        for (pane, expected, name) in [
            (AGENT, 330.0, "Agent"),
            (FILES, 300.0, "Files"),
            (DIFF, 320.0, "Diff"),
            (PREVIEW, 400.0, "Preview"),
        ] {
            assert_close(expected, after[&pane].width, name);
        }
    }

    fn rectangle_end(axis: Axis, rectangle: &Rectangle) -> f32 {
        rectangle_origin(axis, rectangle) + rectangle_extent(axis, rectangle)
    }

    fn assert_close(expected: f32, actual: f32, name: &str) {
        assert!(
            (expected - actual).abs() < 0.01,
            "{name}: expected {expected}, got {actual}"
        );
    }
}
