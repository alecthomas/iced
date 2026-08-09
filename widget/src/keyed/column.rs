//! Keyed columns distribute content vertically while keeping continuity.
use crate::core::layout;
use crate::core::mouse;
use crate::core::overlay;
use crate::core::renderer;
use crate::core::widget::Operation;
use crate::core::widget::tree::{self, Tree};
use crate::core::{
    Alignment, Element, Event, Layout, Length, Padding, Pixels, Rectangle, Shell, Size, Vector,
    Widget,
};

/// A container that distributes its contents vertically while keeping continuity.
///
/// # Example
/// ```no_run
/// # mod iced { pub mod widget { pub use iced_widget::*; } }
/// # pub type State = ();
/// # pub type Element<'a, Message> = iced_widget::core::Element<'a, Message, iced_widget::Theme, iced_widget::Renderer>;
/// use iced::widget::{keyed_column, text};
///
/// enum Message {
///     // ...
/// }
///
/// fn view(state: &State) -> Element<'_, Message> {
///     keyed_column((0..=100).map(|i| {
///         (i, text!("Item {i}").into())
///     })).into()
/// }
/// ```
pub struct Column<'a, Key, Message, Theme = crate::Theme, Renderer = crate::Renderer>
where
    Key: Copy + PartialEq,
{
    spacing: f32,
    padding: Padding,
    width: Length,
    height: Length,
    align_items: Alignment,
    keys: Vec<Key>,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
}

impl<'a, Key, Message, Theme, Renderer> Column<'a, Key, Message, Theme, Renderer>
where
    Key: Copy + PartialEq,
    Renderer: crate::core::Renderer,
{
    /// Creates an empty [`Column`].
    pub fn new() -> Self {
        Self::from_vecs(Vec::new(), Vec::new())
    }

    /// Creates a [`Column`] from already allocated [`Vec`]s.
    ///
    /// Keep in mind that the [`Column`] will not inspect the [`Vec`]s, which means
    /// it won't automatically adapt to the sizing strategy of its contents.
    ///
    /// If any of the children have a [`Length::Fill`] strategy, you will need to
    /// call [`Column::width`] or [`Column::height`] accordingly.
    pub fn from_vecs(keys: Vec<Key>, children: Vec<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            spacing: 0.0,
            padding: Padding::ZERO,
            width: Length::Fit,
            height: Length::Fit,
            align_items: Alignment::Start,
            keys,
            children,
        }
    }

    /// Creates a [`Column`] with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self::from_vecs(Vec::with_capacity(capacity), Vec::with_capacity(capacity))
    }

    /// Creates a [`Column`] with the given elements.
    pub fn with_children(
        children: impl IntoIterator<Item = (Key, Element<'a, Message, Theme, Renderer>)>,
    ) -> Self {
        let iterator = children.into_iter();

        Self::with_capacity(iterator.size_hint().0).extend(iterator)
    }

    /// Sets the vertical spacing _between_ elements.
    ///
    /// Custom margins per element do not exist in iced. You should use this
    /// method instead! While less flexible, it helps you keep spacing between
    /// elements consistent.
    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }

    /// Sets the [`Padding`] of the [`Column`].
    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    /// Sets the width of the [`Column`].
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height of the [`Column`].
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the horizontal alignment of the contents of the [`Column`] .
    pub fn align_items(mut self, align: Alignment) -> Self {
        self.align_items = align;
        self
    }

    /// Adds an element to the [`Column`].
    pub fn push(
        mut self,
        key: Key,
        child: impl Into<Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        let child = child.into();

        if !child.as_widget().is_void() {
            self.keys.push(key);
            self.children.push(child);
        }

        self
    }

    /// Adds an element to the [`Column`], if `Some`.
    pub fn push_maybe(
        self,
        key: Key,
        child: Option<impl Into<Element<'a, Message, Theme, Renderer>>>,
    ) -> Self {
        if let Some(child) = child {
            self.push(key, child)
        } else {
            self
        }
    }

    /// Extends the [`Column`] with the given children.
    pub fn extend(
        self,
        children: impl IntoIterator<Item = (Key, Element<'a, Message, Theme, Renderer>)>,
    ) -> Self {
        children
            .into_iter()
            .fold(self, |column, (key, child)| column.push(key, child))
    }
}

impl<Key, Message, Renderer> Default for Column<'_, Key, Message, Renderer>
where
    Key: Copy + PartialEq,
    Renderer: crate::core::Renderer,
{
    fn default() -> Self {
        Self::new()
    }
}

struct State<Key>
where
    Key: Copy + PartialEq,
{
    keys: Vec<Key>,
}

impl<Key, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Column<'_, Key, Message, Theme, Renderer>
where
    Renderer: crate::core::Renderer,
    Key: Copy + PartialEq + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Key>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            keys: self.keys.clone(),
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let Tree {
            state, children, ..
        } = tree;

        let state = state.downcast_mut::<State<Key>>();

        // Trees must follow their keys: positional pairing hands every
        // slot's state to a neighbor when a windowed list shifts by one.
        let mut previous: Vec<Option<Tree>> =
            std::mem::take(children).into_iter().map(Some).collect();
        *children = self
            .keys
            .iter()
            .zip(&mut self.children)
            .map(|(key, child)| {
                let inherited = state
                    .keys
                    .iter()
                    .position(|previous_key| previous_key == key)
                    .and_then(|index| {
                        previous.get_mut(index).and_then(Option::take)
                    });
                let mut tree = inherited
                    .unwrap_or_else(|| Tree::new(child.as_widget()));
                child.as_widget_mut().diff(&mut tree);
                tree
            })
            .collect();

        if state.keys != self.keys {
            state.keys.clone_from(&self.keys);
        }

        if self.width.is_fit() || self.height.is_fit() {
            for child in &self.children {
                let size = child.as_widget().size();

                self.width = self.width.cross(size.width);
                self.height = self.height.stack(size.height);
            }
        }
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::flex::resolve(
            layout::flex::Axis::Vertical,
            renderer,
            limits,
            self.width,
            self.height,
            self.padding,
            self.spacing,
            self.align_items,
            &mut self.children,
            &mut tree.children,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((child, state), layout)| {
                    child
                        .as_widget_mut()
                        .operate(state, layout, renderer, operation);
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .update(tree, event, layout, cursor, renderer, shell, viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((child, state), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(state, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Key, Message, Theme, Renderer> From<Column<'a, Key, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Key: Copy + PartialEq + 'static,
    Message: 'a,
    Theme: 'a,
    Renderer: crate::core::Renderer + 'a,
{
    fn from(column: Column<'a, Key, Message, Theme, Renderer>) -> Self {
        Self::new(column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::layout::{Limits, Node};
    use crate::core::widget::tree;
    use crate::core::{Theme, mouse};

    /// Carries its construction value as widget state, so a test can tell
    /// an inherited tree from a freshly created one.
    struct Marker(u32);

    impl<Message> Widget<Message, Theme, ()> for Marker {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<u32>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(self.0)
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Shrink)
        }

        fn layout(
            &mut self,
            _tree: &mut Tree,
            _renderer: &(),
            _limits: &Limits,
        ) -> Node {
            Node::new(Size::ZERO)
        }

        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut (),
            _theme: &Theme,
            _style: &renderer::Style,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
        }
    }

    fn column<'a>(
        children: impl IntoIterator<Item = (usize, u32)>,
    ) -> Column<'a, usize, (), Theme, ()> {
        Column::with_children(
            children
                .into_iter()
                .map(|(key, value)| (key, Element::new(Marker(value)))),
        )
    }

    fn states(tree: &Tree) -> Vec<u32> {
        tree.children
            .iter()
            .map(|child| *child.state.downcast_ref::<u32>())
            .collect()
    }

    /// A windowed list shifting by one produces an equal-length child
    /// list with shifted keys; every tree must follow its key.
    #[test]
    fn diff_moves_trees_with_their_keys() {
        let mut initial = column([(1, 1), (2, 2), (3, 3)]);
        let mut tree = Tree::empty();
        tree.state = Widget::<(), Theme, ()>::state(&initial);
        Widget::<(), Theme, ()>::diff(&mut initial, &mut tree);
        assert_eq!(vec![1, 2, 3], states(&tree));

        // Shift the window: key 1 leaves, key 4 enters, same length.
        let mut shifted = column([(2, 9), (3, 9), (4, 4)]);
        Widget::<(), Theme, ()>::diff(&mut shifted, &mut tree);

        assert_eq!(vec![2, 3, 4], states(&tree));
    }
}
