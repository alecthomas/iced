//! Ownership of an in-flight scroll gesture: platforms deliver a whole
//! gesture, momentum included, to the widget it started on.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::time::{Duration, Instant};

/// Longest gap between wheel events that still counts as one gesture: above
/// the finger-lift-to-momentum handoff, below a deliberate re-aim.
const IDLE: Duration = Duration::from_millis(150);

/// Identifies a scroll-handling widget. Every value is distinct; [`Default`]
/// exists so widget states can derive it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Id(u64);

impl Default for Id {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        Id(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

thread_local! {
    static OWNER: Cell<Option<(Id, Instant)>> = const { Cell::new(None) };
}

/// The owner of the gesture still in flight, if any.
fn live() -> Option<Id> {
    OWNER.with(|owner| {
        owner
            .get()
            .filter(|(_, seen)| seen.elapsed() < IDLE)
            .map(|(id, _)| id)
    })
}

/// Whether `id` holds the gesture, and so must keep scrolling wherever the
/// pointer has since travelled.
pub fn owns(id: Id) -> bool {
    live() == Some(id)
}

/// Whether `id` may act on a wheel event: it holds the gesture, or no one
/// does and the pointer is over `id`.
pub fn may_act(id: Id, is_hovered: bool) -> bool {
    match live() {
        Some(held) => held == id,
        None => is_hovered,
    }
}

/// Takes or renews `id`'s hold, only ever after consuming a scroll, so that a
/// widget with nothing left to scroll still chains to its parent.
pub fn claim(id: Id) {
    OWNER.with(|owner| owner.set(Some((id, Instant::now()))));
}

// The latch is thread-local, so each test thread starts with a free gesture.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_keeps_the_gesture_when_the_pointer_moves_to_a_rival() {
        let (owner, rival) = (Id::default(), Id::default());

        assert!(may_act(owner, true));
        claim(owner);

        assert!(!may_act(rival, true));
        assert!(may_act(owner, false));
    }

    #[test]
    fn a_free_gesture_goes_to_whatever_is_hovered() {
        let (hovered, elsewhere) = (Id::default(), Id::default());

        assert!(!may_act(elsewhere, false));
        assert!(may_act(hovered, true));
    }

    #[test]
    fn acting_without_consuming_leaves_the_gesture_free() {
        let (passive, rival) = (Id::default(), Id::default());

        assert!(may_act(passive, true));
        assert!(!owns(passive));
        assert!(may_act(rival, true));
    }

    #[test]
    fn an_idle_gesture_is_released() {
        let (owner, rival) = (Id::default(), Id::default());

        claim(owner);
        std::thread::sleep(IDLE + Duration::from_millis(10));

        assert!(may_act(rival, true));
    }
}
