//! Off-thread text shaping into the paragraph cache.
//!
//! Lookups never shape: cold keys queue for the worker, which owns a
//! private [`cosmic_text::FontSystem`] over a clone of the global font
//! database. fontdb IDs survive the clone, so worker-shaped buffers
//! rasterize on the main instance without any shared lock.
use crate::core::alignment;
use crate::core::text::{Alignment, Ellipsis, LineHeight, Shaping, Span, Text, Wrapping};
use crate::core::{Color, Font, Padding, Pixels, Size};
use crate::text::{self, paragraph};

use std::borrow::Cow;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{LazyLock, Mutex, RwLock, mpsc};
use std::thread;

/// Result of a non-blocking paragraph lookup.
#[derive(Debug, Clone)]
pub enum ParagraphLookup {
    /// The shaped paragraph was warm in the cache.
    Warm(paragraph::Paragraph),
    /// Queued for off-thread shaping; the notifier fires on completion.
    Pending,
}

/// Registers the callback fired after the worker shapes a batch;
/// applications use it to request a redraw.
pub fn set_notifier(notifier: impl Fn() + Send + Sync + 'static) {
    *NOTIFIER.write().expect("Write shaper notifier") = Some(Box::new(notifier));
}

/// Non-blocking cache lookup for plain text.
pub fn lookup_plain(text: &Text<&str>) -> ParagraphLookup {
    let base_key = paragraph::text_base_key(text);
    let key = paragraph::full_key(base_key, text.bounds);
    if let Some(hit) = paragraph::cache::get(key) {
        return ParagraphLookup::Warm(hit);
    }
    if paragraph::cache::try_pend(key) {
        submit(Job {
            content: JobContent::Plain(text.content.to_owned()),
            bounds: text.bounds,
            size: text.size,
            line_height: text.line_height,
            font: text.font,
            align_x: text.align_x,
            align_y: text.align_y,
            shaping: text.shaping,
            wrapping: text.wrapping,
            ellipsis: text.ellipsis,
            hint_factor: text.hint_factor,
            epoch: text::font_epoch(),
        });
    }
    ParagraphLookup::Pending
}

/// Non-blocking cache lookup for rich spans.
pub fn lookup_spans<Link>(text: &Text<&[Span<'_, Link>]>) -> ParagraphLookup {
    let base_key = paragraph::spans_base_key(text);
    let key = paragraph::full_key(base_key, text.bounds);
    if let Some(hit) = paragraph::cache::get(key) {
        return ParagraphLookup::Warm(hit);
    }
    if paragraph::cache::try_pend(key) {
        submit(Job {
            content: JobContent::Spans(
                text.content
                    .iter()
                    .map(|span| OwnedSpan {
                        text: span.text.to_string(),
                        size: span.size,
                        line_height: span.line_height,
                        font: span.font,
                        color: span.color,
                    })
                    .collect(),
            ),
            bounds: text.bounds,
            size: text.size,
            line_height: text.line_height,
            font: text.font,
            align_x: text.align_x,
            align_y: text.align_y,
            shaping: text.shaping,
            wrapping: text.wrapping,
            ellipsis: text.ellipsis,
            hint_factor: text.hint_factor,
            epoch: text::font_epoch(),
        });
    }
    ParagraphLookup::Pending
}

static NOTIFIER: RwLock<Option<Box<dyn Fn() + Send + Sync>>> = RwLock::new(None);

struct Job {
    content: JobContent,
    bounds: Size,
    size: Pixels,
    line_height: LineHeight,
    font: Font,
    align_x: Alignment,
    align_y: alignment::Vertical,
    shaping: Shaping,
    wrapping: Wrapping,
    ellipsis: Ellipsis,
    hint_factor: Option<f32>,
    /// Jobs from a flushed font epoch are dropped, not shaped stale.
    epoch: u64,
}

enum JobContent {
    Plain(String),
    Spans(Vec<OwnedSpan>),
}

struct OwnedSpan {
    text: String,
    size: Option<Pixels>,
    line_height: Option<LineHeight>,
    font: Option<Font>,
    color: Option<Color>,
}

fn submit(job: Job) {
    let sender = WORKER.lock().expect("Lock shaper sender");
    let _ = sender.send(job);
}

static WORKER: LazyLock<Mutex<Sender<Job>>> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("iced-text-shaper".into())
        .spawn(move || run(receiver));
    Mutex::new(sender)
});

fn run(receiver: Receiver<Job>) {
    let mut fonts: Option<PrivateFonts> = None;
    while let Ok(job) = receiver.recv() {
        let mut jobs = vec![job];
        // Drain the burst so one notification covers all of it.
        while let Ok(job) = receiver.try_recv() {
            jobs.push(job);
        }

        let mut shaped = false;
        for job in jobs {
            let epoch = text::font_epoch();
            if job.epoch != epoch {
                continue;
            }
            let fonts = match &mut fonts {
                Some(fonts) if fonts.epoch == epoch => fonts,
                _ => fonts.insert(PrivateFonts::clone_global(epoch)),
            };
            let (key, paragraph) = shape(&job, fonts);
            // Discard work from a mid-shape font load; the flush already
            // cleared the pending key, so it will re-queue.
            if text::font_epoch() != epoch {
                continue;
            }
            paragraph::cache::insert(key, &paragraph);
            shaped = true;
        }

        if shaped && let Some(notifier) = NOTIFIER.read().expect("Read shaper notifier").as_ref() {
            notifier();
        }
    }
}

struct PrivateFonts {
    epoch: u64,
    version: text::Version,
    raw: cosmic_text::FontSystem,
}

impl PrivateFonts {
    /// Clones the global font database (metadata only; font bytes are
    /// Arc-shared and fontdb IDs survive the clone).
    fn clone_global(epoch: u64) -> Self {
        let global = text::font_system().read().expect("Read font system");
        Self {
            epoch,
            version: global.version(),
            raw: cosmic_text::FontSystem::new_with_locale_and_db(
                global.raw.locale().to_owned(),
                global.raw.db().clone(),
            ),
        }
    }
}

fn shape(job: &Job, fonts: &mut PrivateFonts) -> (u64, paragraph::Paragraph) {
    match &job.content {
        JobContent::Plain(content) => {
            let text = Text {
                content: content.as_str(),
                bounds: job.bounds,
                size: job.size,
                line_height: job.line_height,
                font: job.font,
                align_x: job.align_x,
                align_y: job.align_y,
                shaping: job.shaping,
                wrapping: job.wrapping,
                ellipsis: job.ellipsis,
                hint_factor: job.hint_factor,
            };
            let base_key = paragraph::text_base_key(&text);
            (
                paragraph::full_key(base_key, job.bounds),
                paragraph::shape_plain(&mut fonts.raw, fonts.version, &text, base_key),
            )
        }
        JobContent::Spans(owned) => {
            let spans = owned
                .iter()
                .map(|span| Span {
                    text: Cow::Borrowed(span.text.as_str()),
                    size: span.size,
                    line_height: span.line_height,
                    font: span.font,
                    color: span.color,
                    link: None::<()>,
                    highlight: None,
                    padding: Padding::ZERO,
                    underline: false,
                    underline_offset: None,
                    strikethrough: false,
                })
                .collect::<Vec<_>>();
            let text = Text {
                content: spans.as_slice(),
                bounds: job.bounds,
                size: job.size,
                line_height: job.line_height,
                font: job.font,
                align_x: job.align_x,
                align_y: job.align_y,
                shaping: job.shaping,
                wrapping: job.wrapping,
                ellipsis: job.ellipsis,
                hint_factor: job.hint_factor,
            };
            let base_key = paragraph::spans_base_key(&text);
            (
                paragraph::full_key(base_key, job.bounds),
                paragraph::shape_spans(&mut fonts.raw, fonts.version, &text, base_key),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::text::Paragraph as _;
    use std::time::{Duration, Instant};

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

    #[test]
    fn test_worker_shapes_cold_lookups_off_thread() {
        let _guard = crate::text::tests::FONT_MUTATION_GUARD
            .lock()
            .expect("Lock font mutations");
        let (sender, receiver) = mpsc::channel();
        set_notifier(move || {
            let _ = sender.send(());
        });

        let text = plain("shaped off the main thread", 240.0);
        assert!(matches!(lookup_plain(&text), ParagraphLookup::Pending));

        let deadline = Instant::now() + Duration::from_secs(10);
        let warm = loop {
            let _ = receiver
                .recv_timeout(deadline - Instant::now())
                .expect("Worker must notify");
            if let ParagraphLookup::Warm(warm) = lookup_plain(&text) {
                break warm;
            }
        };

        // The blocking constructor must reuse the worker's shape.
        let blocking = paragraph::Paragraph::with_text(plain("shaped off the main thread", 240.0));
        assert!(
            warm.ptr_eq(&blocking),
            "the blocking constructor must reuse the worker's shape"
        );

        set_notifier(|| {});
    }
}
