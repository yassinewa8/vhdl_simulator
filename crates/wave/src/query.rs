//! The questions the waveform viewer asks every frame.
//!
//! All of them work on a signal's list of change times with binary search
//! (halving the list until the answer is found), so a signal with 10
//! million changes needs about 24 steps per lookup.
//!
//! Time windows include their start and exclude their end: a change at
//! exactly the end of one window belongs to the next.

use std::ops::Range;

use studio_core::{SimTime, Value};

use crate::waveform::{SignalId, SignalView, Waveform};

/// A signal changing to `value` at `time`.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub time: SimTime,
    pub value: Value,
}

/// A signal's activity in a time window.
#[derive(Debug, Clone, PartialEq)]
pub struct Changes {
    /// The last change before the window: the value the window starts
    /// with. `None` if the signal has no value yet at the window start.
    pub before: Option<Change>,
    /// Every change inside the window, in time order.
    pub changes: Vec<Change>,
}

/// What happens to a signal within one pixel column of the wave display.
#[derive(Debug, Clone, PartialEq)]
pub enum Bucket {
    /// No change: the previous value carries on.
    Quiet,
    /// Exactly one change.
    One(Change),
    /// Several changes, too close to draw separately ("many changes here").
    Many {
        count: usize,
        /// The value after the last change, which carries on afterwards.
        last_value: Value,
    },
}

/// A signal's activity summarised per pixel column.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// The value just before the window starts.
    pub start: Option<Value>,
    /// One bucket per pixel column, left to right.
    pub buckets: Vec<Bucket>,
}

impl Waveform {
    /// The value just before `window` and every change inside it.
    /// `None` if the signal isn't loaded.
    pub fn changes(&self, id: SignalId, window: Range<SimTime>) -> Option<Changes> {
        let view = self.view(id)?;
        let first = view.count_before(window.start);
        let end = view.count_before(window.end).max(first);
        Some(Changes {
            before: first.checked_sub(1).map(|change| change_at(&view, change)),
            changes: (first..end).map(|change| change_at(&view, change)).collect(),
        })
    }

    /// `window` cut into `pixels` equal columns, each summarised as quiet,
    /// one change, or many changes. For drawing zoomed-out views without
    /// visiting every change. `None` if the signal isn't loaded.
    pub fn summary(&self, id: SignalId, window: Range<SimTime>, pixels: usize) -> Option<Summary> {
        let view = self.view(id)?;
        let first = view.count_before(window.start);
        let start = first.checked_sub(1).map(|change| view.value(change));

        // Changes before each column boundary, found by binary search, so
        // column i holds changes number counts[i] up to counts[i + 1].
        let counts: Vec<usize> = (0..=pixels)
            .map(|column| view.count_before(column_edge(&window, column, pixels)))
            .collect();
        let buckets = counts
            .windows(2)
            .map(|pair| match pair[1] - pair[0] {
                0 => Bucket::Quiet,
                1 => Bucket::One(change_at(&view, pair[0])),
                count => Bucket::Many {
                    count,
                    last_value: view.value(pair[1] - 1),
                },
            })
            .collect();
        Some(Summary { start, buckets })
    }

    /// The first change strictly after `time`.
    pub fn next_change(&self, id: SignalId, time: SimTime) -> Option<SimTime> {
        let view = self.view(id)?;
        let next = view.count_up_to(time);
        (next < view.len()).then(|| view.time(next))
    }

    /// The last change strictly before `time`.
    pub fn prev_change(&self, id: SignalId, time: SimTime) -> Option<SimTime> {
        let view = self.view(id)?;
        let previous = view.count_before(time).checked_sub(1)?;
        Some(view.time(previous))
    }

    /// The first time strictly after `time` that the signal becomes
    /// `value`. Values must match exactly (`H` is not `1`).
    ///
    /// Unlike the other queries this looks at each change in turn (in
    /// place, without copying values), so it's slower on a busy signal
    /// when the value is far away: up to about 0.4 s per 10 million changes
    /// (measured with `cargo run --release -p wave --example bench`).
    pub fn next_equal(&self, id: SignalId, time: SimTime, value: &Value) -> Option<SimTime> {
        let view = self.view(id)?;
        (view.count_up_to(time)..view.len())
            .find(|&change| view.value_is(change, value))
            .map(|change| view.time(change))
    }

    /// The last time strictly before `time` that the signal became `value`.
    pub fn prev_equal(&self, id: SignalId, time: SimTime, value: &Value) -> Option<SimTime> {
        let view = self.view(id)?;
        (0..view.count_before(time))
            .rev()
            .find(|&change| view.value_is(change, value))
            .map(|change| view.time(change))
    }
}

fn change_at(view: &SignalView<'_>, change: usize) -> Change {
    Change {
        time: view.time(change),
        value: view.value(change),
    }
}

/// The time at the left edge of column `column` (or the window end, for
/// `column == pixels`). Worked out in 128-bit arithmetic so long windows
/// in femtoseconds can't overflow.
fn column_edge(window: &Range<SimTime>, column: usize, pixels: usize) -> SimTime {
    let start = u128::from(window.start.femtoseconds);
    let length = u128::from(window.end.femtoseconds.saturating_sub(window.start.femtoseconds));
    let offset = length * column as u128 / pixels.max(1) as u128;
    SimTime::from_femtoseconds((start + offset) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(value: u64) -> SimTime {
        SimTime::from_femtoseconds(value * 1_000_000)
    }

    #[test]
    fn column_edges_split_the_window_evenly() {
        let window = ns(100)..ns(200);
        let edges: Vec<SimTime> = (0..=4).map(|i| column_edge(&window, i, 4)).collect();
        assert_eq!(edges, [ns(100), ns(125), ns(150), ns(175), ns(200)]);
    }

    #[test]
    fn column_edges_survive_huge_windows() {
        // Close to the largest time a u64 of femtoseconds can hold.
        let window = SimTime::ZERO..SimTime::from_femtoseconds(u64::MAX);
        assert_eq!(column_edge(&window, 1000, 1000), SimTime::from_femtoseconds(u64::MAX));
        assert_eq!(column_edge(&window, 0, 0), SimTime::ZERO, "no columns at all");
    }
}
