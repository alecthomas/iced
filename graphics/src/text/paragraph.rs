//! Draw paragraphs.
use crate::core;
use crate::core::alignment;
use crate::core::text::{Alignment, Ellipsis, Hit, LineHeight, Shaping, Span, Text, Wrapping};
use crate::core::{Font, Pixels, Point, Rectangle, Size};
use crate::text;

use rustc_hash::FxHasher;

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{self, Arc};

/// A bunch of text.
#[derive(Clone, PartialEq)]
pub struct Paragraph(Arc<Internal>);

#[derive(Clone)]
struct Internal {
    buffer: cosmic_text::Buffer,
    /// Cache key of everything but `bounds`; lets `resize` re-key without
    /// the original spans. Zero opts out (default/empty paragraphs).
    base_key: u64,
    font: Font,
    shaping: Shaping,
    wrapping: Wrapping,
    ellipsis: Ellipsis,
    align_x: Alignment,
    align_y: alignment::Vertical,
    bounds: Size,
    min_bounds: Size,
    version: text::Version,
    hint: bool,
    hint_factor: f32,
}

impl Paragraph {
    /// Creates a new empty [`Paragraph`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the buffer of the [`Paragraph`].
    pub fn buffer(&self) -> &cosmic_text::Buffer {
        &self.internal().buffer
    }

    /// Creates a [`Weak`] reference to the [`Paragraph`].
    ///
    /// This is useful to avoid cloning the [`Paragraph`] when
    /// referential guarantees are unnecessary. For instance,
    /// when creating a rendering tree.
    pub fn downgrade(&self) -> Weak {
        let paragraph = self.internal();

        Weak {
            raw: Arc::downgrade(paragraph),
            min_bounds: paragraph.min_bounds,
            align_x: paragraph.align_x,
            align_y: paragraph.align_y,
        }
    }

    fn internal(&self) -> &Arc<Internal> {
        &self.0
    }

    /// Distinguishes a shared cache hit from an equal re-shape.
    #[cfg(test)]
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl core::text::Paragraph for Paragraph {
    type Font = Font;

    fn with_text(text: Text<&str>) -> Self {
        let base_key = text_base_key(&text);
        if let Some(hit) = cache::get(full_key(base_key, text.bounds)) {
            return hit;
        }

        let mut font_system = text::font_system().write().expect("Write font system");
        let version = font_system.version();
        let paragraph = shape_plain(font_system.raw(), version, &text, base_key);
        cache::insert(full_key(base_key, text.bounds), &paragraph);
        paragraph
    }

    fn with_spans<Link>(text: Text<&[Span<'_, Link>]>) -> Self {
        let base_key = spans_base_key(&text);
        if let Some(hit) = cache::get(full_key(base_key, text.bounds)) {
            return hit;
        }

        let mut font_system = text::font_system().write().expect("Write font system");
        let version = font_system.version();
        let paragraph = shape_spans(font_system.raw(), version, &text, base_key);
        cache::insert(full_key(base_key, text.bounds), &paragraph);
        paragraph
    }

    fn try_with_spans<Link>(text: Text<&[Span<'_, Link>]>) -> Option<Self> {
        match crate::text::shaper::lookup_spans(&text) {
            crate::text::shaper::ParagraphLookup::Warm(paragraph) => Some(paragraph),
            crate::text::shaper::ParagraphLookup::Pending => None,
        }
    }

    fn resize(&mut self, new_bounds: Size) {
        let base_key = self.0.base_key;
        // Bounds are part of the cache key, so a resize is a lookup under
        // the new bounds — oscillating widths re-shape nothing when warm.
        if base_key != 0
            && let Some(hit) = cache::get(full_key(base_key, new_bounds))
        {
            *self = hit;
            return;
        }

        let paragraph = Arc::make_mut(&mut self.0);

        let mut font_system = text::font_system().write().expect("Write font system");

        paragraph.buffer.set_size(
            Some(new_bounds.width * paragraph.hint_factor),
            Some(new_bounds.height * paragraph.hint_factor),
        );
        paragraph
            .buffer
            .shape_until_scroll(font_system.raw(), false);

        let min_bounds = text::align(&mut paragraph.buffer, font_system.raw(), paragraph.align_x)
            / paragraph.hint_factor;

        paragraph.bounds = new_bounds;
        paragraph.min_bounds = min_bounds;

        drop(font_system);
        if base_key != 0 {
            cache::insert(full_key(base_key, new_bounds), self);
        }
    }

    fn compare(&self, text: Text<()>) -> core::text::Difference {
        let font_system = text::font_system().read().expect("Read font system");
        let paragraph = self.internal();
        let metrics = paragraph.buffer.metrics();

        if paragraph.version != font_system.version
            || metrics.font_size != text.size.0 * paragraph.hint_factor
            || metrics.line_height
                != text.line_height.to_absolute(text.size).0 * paragraph.hint_factor
            || paragraph.font != text.font
            || paragraph.shaping != text.shaping
            || paragraph.wrapping != text.wrapping
            || paragraph.ellipsis != text.ellipsis
            || paragraph.align_x != text.align_x
            || paragraph.align_y != text.align_y
            || paragraph.hint.then_some(paragraph.hint_factor)
                != text::hint_factor(text.size, text.hint_factor)
        {
            core::text::Difference::Shape
        } else if paragraph.bounds != text.bounds {
            core::text::Difference::Bounds
        } else {
            core::text::Difference::None
        }
    }

    fn hint_factor(&self) -> Option<f32> {
        self.0.hint.then_some(self.0.hint_factor)
    }

    fn size(&self) -> Pixels {
        Pixels(self.0.buffer.metrics().font_size / self.0.hint_factor)
    }

    fn font(&self) -> Font {
        self.0.font
    }

    fn line_height(&self) -> LineHeight {
        LineHeight::Absolute(Pixels(
            self.0.buffer.metrics().line_height / self.0.hint_factor,
        ))
    }

    fn align_x(&self) -> Alignment {
        self.internal().align_x
    }

    fn align_y(&self) -> alignment::Vertical {
        self.internal().align_y
    }

    fn wrapping(&self) -> Wrapping {
        self.0.wrapping
    }

    fn ellipsis(&self) -> Ellipsis {
        self.0.ellipsis
    }

    fn shaping(&self) -> Shaping {
        self.0.shaping
    }

    fn bounds(&self) -> Size {
        self.0.bounds
    }

    fn min_bounds(&self) -> Size {
        self.internal().min_bounds
    }

    fn hit_test(&self, point: Point) -> Option<Hit> {
        let cursor = self
            .internal()
            .buffer
            .hit(point.x * self.0.hint_factor, point.y * self.0.hint_factor)?;

        Some(Hit::CharOffset(cursor.index))
    }

    fn hit_span(&self, point: Point) -> Option<usize> {
        let internal = self.internal();

        let cursor = internal
            .buffer
            .hit(point.x * self.0.hint_factor, point.y * self.0.hint_factor)?;
        let line = internal.buffer.lines.get(cursor.line)?;

        if cursor.index >= line.text().len() {
            return None;
        }

        let index = match cursor.affinity {
            cosmic_text::Affinity::Before => cursor.index.saturating_sub(1),
            cosmic_text::Affinity::After => cursor.index,
        };

        let mut hit = None;
        let glyphs = line
            .layout_opt()
            .as_ref()?
            .iter()
            .flat_map(|line| line.glyphs.iter());

        for glyph in glyphs {
            if glyph.start <= index && index < glyph.end {
                hit = Some(glyph);
                break;
            }
        }

        Some(hit?.metadata)
    }

    fn span_bounds(&self, index: usize) -> Vec<Rectangle> {
        let internal = self.internal();

        let mut bounds = Vec::new();
        let mut current_bounds = None;

        let glyphs = internal
            .buffer
            .layout_runs()
            .flat_map(|run| {
                let line_top = run.line_top;
                let line_height = run.line_height;

                run.glyphs
                    .iter()
                    .map(move |glyph| (line_top, line_height, glyph))
            })
            .skip_while(|(_, _, glyph)| glyph.metadata != index)
            .take_while(|(_, _, glyph)| glyph.metadata == index);

        for (line_top, line_height, glyph) in glyphs {
            let y = line_top + glyph.y;

            let new_bounds = || {
                Rectangle::new(
                    Point::new(glyph.x, y),
                    Size::new(glyph.w, glyph.line_height_opt.unwrap_or(line_height)),
                ) * (1.0 / self.0.hint_factor)
            };

            match current_bounds.as_mut() {
                None => {
                    current_bounds = Some(new_bounds());
                }
                Some(current_bounds) if y != current_bounds.y => {
                    bounds.push(*current_bounds);
                    *current_bounds = new_bounds();
                }
                Some(current_bounds) => {
                    current_bounds.width += glyph.w / self.0.hint_factor;
                }
            }
        }

        bounds.extend(current_bounds);
        bounds
    }
}

impl Default for Paragraph {
    fn default() -> Self {
        Self(Arc::new(Internal::default()))
    }
}

/// Shapes plain text with `font_system`; callable off the main thread
/// with a private [`cosmic_text::FontSystem`].
pub(crate) fn shape_plain(
    font_system: &mut cosmic_text::FontSystem,
    version: text::Version,
    text: &Text<&str>,
    base_key: u64,
) -> Paragraph {
    log::trace!("Allocating plain paragraph: {}", text.content);

    let (hint, hint_factor) = match text::hint_factor(text.size, text.hint_factor) {
        Some(hint_factor) => (true, hint_factor),
        _ => (false, 1.0),
    };

    let mut buffer = cosmic_text::Buffer::new(
        font_system,
        cosmic_text::Metrics::new(
            f32::from(text.size) * hint_factor,
            f32::from(text.line_height.to_absolute(text.size)) * hint_factor,
        ),
    );

    if hint {
        buffer.set_hinting(cosmic_text::Hinting::Enabled);
    }

    buffer.set_size(
        Some(text.bounds.width * hint_factor),
        Some(text.bounds.height * hint_factor),
    );

    buffer.set_wrap(text::to_wrap(text.wrapping));
    buffer.set_ellipsize(text::to_ellipsize(
        text.ellipsis,
        text.bounds.height * hint_factor,
    ));

    buffer.set_text(
        text.content,
        &text::to_attributes(text.font),
        text::to_shaping(text.shaping, text.content),
        None,
    );
    buffer.shape_until_scroll(font_system, false);

    let min_bounds = text::align(&mut buffer, font_system, text.align_x) / hint_factor;

    Paragraph(Arc::new(Internal {
        buffer,
        base_key,
        hint,
        hint_factor,
        font: text.font,
        align_x: text.align_x,
        align_y: text.align_y,
        shaping: text.shaping,
        wrapping: text.wrapping,
        ellipsis: text.ellipsis,
        bounds: text.bounds,
        min_bounds,
        version,
    }))
}

/// Shapes rich spans with `font_system`; callable off the main thread
/// with a private [`cosmic_text::FontSystem`].
pub(crate) fn shape_spans<Link>(
    font_system: &mut cosmic_text::FontSystem,
    version: text::Version,
    text: &Text<&[Span<'_, Link>]>,
    base_key: u64,
) -> Paragraph {
    log::trace!("Allocating rich paragraph: {} spans", text.content.len());

    let (hint, hint_factor) = match text::hint_factor(text.size, text.hint_factor) {
        Some(hint_factor) => (true, hint_factor),
        _ => (false, 1.0),
    };

    let mut buffer = cosmic_text::Buffer::new(
        font_system,
        cosmic_text::Metrics::new(
            f32::from(text.size) * hint_factor,
            f32::from(text.line_height.to_absolute(text.size)) * hint_factor,
        ),
    );

    if hint {
        buffer.set_hinting(cosmic_text::Hinting::Enabled);
    }

    buffer.set_size(
        Some(text.bounds.width * hint_factor),
        Some(text.bounds.height * hint_factor),
    );

    buffer.set_wrap(text::to_wrap(text.wrapping));
    buffer.set_ellipsize(text::to_ellipsize(
        text.ellipsis,
        text.bounds.height * hint_factor,
    ));

    buffer.set_rich_text(
        text.content.iter().enumerate().map(|(i, span)| {
            let attrs = text::to_attributes(span.font.unwrap_or(text.font));

            let attrs = match (span.size, span.line_height) {
                (None, None) => attrs,
                _ => {
                    let size = span.size.unwrap_or(text.size);

                    attrs.metrics(cosmic_text::Metrics::new(
                        f32::from(size) * hint_factor,
                        f32::from(
                            span.line_height
                                .unwrap_or(text.line_height)
                                .to_absolute(size),
                        ) * hint_factor,
                    ))
                }
            };

            let attrs = if let Some(color) = span.color {
                attrs.color(text::to_color(color))
            } else {
                attrs
            };

            (span.text.as_ref(), attrs.metadata(i))
        }),
        &text::to_attributes(text.font),
        cosmic_text::Shaping::Advanced,
        None,
    );

    buffer.shape_until_scroll(font_system, false);

    let min_bounds = text::align(&mut buffer, font_system, text.align_x) / hint_factor;

    Paragraph(Arc::new(Internal {
        buffer,
        base_key,
        hint,
        hint_factor,
        font: text.font,
        align_x: text.align_x,
        align_y: text.align_y,
        shaping: text.shaping,
        wrapping: text.wrapping,
        ellipsis: text.ellipsis,
        bounds: text.bounds,
        min_bounds,
        version,
    }))
}

pub(crate) fn full_key(base_key: u64, bounds: Size) -> u64 {
    let mut hasher = FxHasher::default();
    base_key.hash(&mut hasher);
    bounds.width.to_bits().hash(&mut hasher);
    bounds.height.to_bits().hash(&mut hasher);
    hasher.finish()
}

/// Hashes every shaping-relevant input of `text` except its content and
/// bounds; content is hashed by the callers, bounds by [`full_key`].
fn hash_common<Content>(hasher: &mut FxHasher, text: &Text<Content>) {
    text.size.0.to_bits().hash(hasher);
    text.line_height
        .to_absolute(text.size)
        .0
        .to_bits()
        .hash(hasher);
    text.font.hash(hasher);
    text.align_x.hash(hasher);
    (text.align_y as u8).hash(hasher);
    text.shaping.hash(hasher);
    text.wrapping.hash(hasher);
    text.ellipsis.hash(hasher);
    match text::hint_factor(text.size, text.hint_factor) {
        Some(factor) => {
            1u8.hash(hasher);
            factor.to_bits().hash(hasher);
        }
        None => 0u8.hash(hasher),
    }
}

fn hash_color(hasher: &mut FxHasher, color: Option<crate::core::Color>) {
    match color {
        Some(color) => {
            1u8.hash(hasher);
            color.r.to_bits().hash(hasher);
            color.g.to_bits().hash(hasher);
            color.b.to_bits().hash(hasher);
            color.a.to_bits().hash(hasher);
        }
        None => 0u8.hash(hasher),
    }
}

pub(crate) fn spans_base_key<Link>(text: &Text<&[Span<'_, Link>]>) -> u64 {
    let mut hasher = FxHasher::default();
    // Rich and plain keyspaces must not collide: identical content shapes
    // differently (span metadata, forced advanced shaping).
    1u8.hash(&mut hasher);
    hash_common(&mut hasher, text);
    text.content.len().hash(&mut hasher);
    for span in text.content {
        span.text.hash(&mut hasher);
        let size = span.size.unwrap_or(text.size);
        size.0.to_bits().hash(&mut hasher);
        span.line_height
            .unwrap_or(text.line_height)
            .to_absolute(size)
            .0
            .to_bits()
            .hash(&mut hasher);
        span.font.unwrap_or(text.font).hash(&mut hasher);
        hash_color(&mut hasher, span.color);
    }
    hasher.finish()
}

pub(crate) fn text_base_key(text: &Text<&str>) -> u64 {
    let mut hasher = FxHasher::default();
    2u8.hash(&mut hasher);
    hash_common(&mut hasher, text);
    text.content.hash(&mut hasher);
    hasher.finish()
}

/// Two-generation cache keying shaped paragraphs by content, so shaping
/// survives widget-tree rebuilds instead of dying with positional state.
pub(crate) mod cache {
    use super::Paragraph;
    use crate::text;

    use rustc_hash::{FxHashMap, FxHashSet};
    use std::sync::{LazyLock, Mutex};
    use std::time::{Duration, Instant};

    /// Generations rotate no faster than this, so paragraphs shaped
    /// ahead of use (prewarming) survive until their first draw.
    const ROTATION_INTERVAL: Duration = Duration::from_secs(1);
    /// Early-rotation bound on a single generation.
    const GENERATION_CAP: usize = 8192;

    struct Generations {
        current: FxHashMap<u64, Paragraph>,
        previous: FxHashMap<u64, Paragraph>,
        /// Keys queued for off-thread shaping; blocks duplicate queueing
        /// while a cold paragraph is in flight.
        pending: FxHashSet<u64>,
        epoch: u64,
        rotated: Instant,
    }

    impl Generations {
        fn rotate(&mut self) {
            self.previous = std::mem::take(&mut self.current);
            self.rotated = Instant::now();
        }

        /// Font loads invalidate every shaped run, including queued ones:
        /// the worker discards jobs from a stale epoch, so their keys must
        /// re-queue rather than stay pending forever.
        fn flush_stale(&mut self) {
            let epoch = text::font_epoch();
            if epoch != self.epoch {
                self.current.clear();
                self.previous.clear();
                self.pending.clear();
                self.epoch = epoch;
            }
        }
    }

    /// A `Paragraph` in a global `Mutex` is also the compile-time proof
    /// that shaped buffers can cross threads.
    static CACHE: LazyLock<Mutex<Generations>> = LazyLock::new(|| {
        Mutex::new(Generations {
            current: FxHashMap::default(),
            previous: FxHashMap::default(),
            pending: FxHashSet::default(),
            epoch: text::font_epoch(),
            rotated: Instant::now(),
        })
    });

    pub(crate) fn get(key: u64) -> Option<Paragraph> {
        let mut cache = CACHE.lock().expect("Lock paragraph cache");
        cache.flush_stale();
        if let Some(hit) = cache.current.get(&key) {
            return Some(hit.clone());
        }
        let hit = cache.previous.remove(&key)?;
        let _ = cache.current.insert(key, hit.clone());
        Some(hit)
    }

    pub(crate) fn insert(key: u64, paragraph: &Paragraph) {
        let mut cache = CACHE.lock().expect("Lock paragraph cache");
        cache.flush_stale();
        if cache.current.len() >= GENERATION_CAP {
            cache.rotate();
        }
        let _ = cache.pending.remove(&key);
        let _ = cache.current.insert(key, paragraph.clone());
    }

    /// Marks `key` as queued for off-thread shaping; false when a warm or
    /// in-flight entry makes queueing redundant.
    pub(crate) fn try_pend(key: u64) -> bool {
        let mut cache = CACHE.lock().expect("Lock paragraph cache");
        cache.flush_stale();
        if cache.current.contains_key(&key) || cache.previous.contains_key(&key) {
            return false;
        }
        cache.pending.insert(key)
    }

    pub(super) fn trim() {
        let mut cache = CACHE.lock().expect("Lock paragraph cache");
        if cache.rotated.elapsed() >= ROTATION_INTERVAL {
            cache.rotate();
        }
    }

    #[cfg(test)]
    pub(super) fn rotate_now() {
        CACHE.lock().expect("Lock paragraph cache").rotate();
    }
}

/// Ages the paragraph cache; renderers call this once per frame and the
/// internal interval gate turns that into a coarse recency window.
pub fn trim_cache() {
    cache::trim();
}

impl fmt::Debug for Paragraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let paragraph = self.internal();

        f.debug_struct("Paragraph")
            .field("font", &paragraph.font)
            .field("shaping", &paragraph.shaping)
            .field("horizontal_alignment", &paragraph.align_x)
            .field("vertical_alignment", &paragraph.align_y)
            .field("bounds", &paragraph.bounds)
            .field("min_bounds", &paragraph.min_bounds)
            .finish()
    }
}

impl PartialEq for Internal {
    fn eq(&self, other: &Self) -> bool {
        self.font == other.font
            && self.shaping == other.shaping
            && self.align_x == other.align_x
            && self.align_y == other.align_y
            && self.bounds == other.bounds
            && self.min_bounds == other.min_bounds
            && self.buffer.metrics() == other.buffer.metrics()
    }
}

impl Default for Internal {
    fn default() -> Self {
        Self {
            buffer: cosmic_text::Buffer::new_empty(cosmic_text::Metrics {
                font_size: 1.0,
                line_height: 1.0,
            }),
            base_key: 0,
            font: Font::default(),
            shaping: Shaping::default(),
            wrapping: Wrapping::default(),
            ellipsis: Ellipsis::default(),
            align_x: Alignment::Default,
            align_y: alignment::Vertical::Top,
            bounds: Size::ZERO,
            min_bounds: Size::ZERO,
            version: text::Version::default(),
            hint: false,
            hint_factor: 1.0,
        }
    }
}

/// A weak reference to a [`Paragraph`].
#[derive(Debug, Clone)]
pub struct Weak {
    raw: sync::Weak<Internal>,
    /// The minimum bounds of the [`Paragraph`].
    pub min_bounds: Size,
    /// The horizontal alignment of the [`Paragraph`].
    pub align_x: Alignment,
    /// The vertical alignment of the [`Paragraph`].
    pub align_y: alignment::Vertical,
}

impl Weak {
    /// Tries to update the reference into a [`Paragraph`].
    pub fn upgrade(&self) -> Option<Paragraph> {
        self.raw.upgrade().map(Paragraph)
    }
}

impl PartialEq for Weak {
    fn eq(&self, other: &Self) -> bool {
        match (self.raw.upgrade(), other.raw.upgrade()) {
            (Some(p1), Some(p2)) => p1 == p2,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::text::Paragraph as _;

    fn plain(content: &str, width: f32) -> Text<&str> {
        Text {
            content,
            bounds: Size::new(width, f32::MAX),
            size: Pixels(14.0),
            line_height: LineHeight::default(),
            font: Font::default(),
            align_x: Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: Shaping::Basic,
            wrapping: Wrapping::default(),
            ellipsis: Ellipsis::default(),
            hint_factor: None,
        }
    }

    // One test function: the cache and font-system version are process
    // globals, and parallel tests would interleave rotations and flushes.
    #[test]
    fn test_cache_identity_eviction_and_invalidation() {
        let _guard = crate::text::tests::FONT_MUTATION_GUARD
            .lock()
            .expect("Lock font mutations");
        let first = Paragraph::with_text(plain("cached paragraph", 320.0));
        let warm = Paragraph::with_text(plain("cached paragraph", 320.0));
        assert!(first.ptr_eq(&warm), "identical content must share");

        let other = Paragraph::with_text(plain("different content", 320.0));
        assert!(!first.ptr_eq(&other), "distinct content must not share");

        let narrow = Paragraph::with_text(plain("cached paragraph", 100.0));
        assert!(!first.ptr_eq(&narrow), "wrap width is part of identity");

        let mut resized = narrow.clone();
        resized.resize(Size::new(320.0, f32::MAX));
        assert!(
            first.ptr_eq(&resized),
            "resize to a cached width must reuse the cached shape"
        );

        cache::rotate_now();
        cache::rotate_now();
        let cold = Paragraph::with_text(plain("cached paragraph", 320.0));
        assert!(!first.ptr_eq(&cold), "two untouched generations must evict");

        let before_bump = Paragraph::with_text(plain("cached paragraph", 320.0));
        assert!(cold.ptr_eq(&before_bump));
        text::font_system()
            .write()
            .expect("Write font system")
            .load_font(std::borrow::Cow::Owned(Vec::new()));
        let after_bump = Paragraph::with_text(plain("cached paragraph", 320.0));
        assert!(
            !cold.ptr_eq(&after_bump),
            "a font-system version bump must flush the cache"
        );
    }
}
