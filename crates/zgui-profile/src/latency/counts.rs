//! A frame's work counters, written into the trace beside its stage marks.
//!
//! The marks say how long each stage took; the counters say how much it did (elements restyled,
//! nodes laid out, runs shaped, fragments rebuilt, draws). A trace reader divides one by the other
//! to tell a frame doing too much from a frame whose every piece costs too much. Counters are
//! process-wide, so a frame's note holds what every window did since the last note.

use std::cell::RefCell;

use crate::counter::{COUNTERS_ENABLED, Counters, snapshot};
use crate::latency::sink::{listening, note_with};

thread_local! {
    /// The counters at the last note.
    static LAST: RefCell<Option<Counters>> = const { RefCell::new(None) };
}

/// Notes the counters that moved since the last call as `f.counts name=value ...`, while a trace
/// listens and the counters are compiled in (the `counters` feature, or a debug build). The first
/// call only takes the baseline.
pub fn note_counters() {
    if !COUNTERS_ENABLED || !listening() {
        return;
    }
    let now = snapshot();
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if let Some(before) = last.as_ref() {
            let moved = before.delta(&now);
            note_with("f.counts", || {
                moved
                    .iter()
                    .filter(|&(_, value)| value != 0)
                    .map(|(counter, value)| format!("{}={value}", counter.name()))
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        }
        *last = Some(now);
    });
}
