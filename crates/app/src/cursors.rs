//! Cursors and named markers in the wave view, like the cursors on an
//! oscilloscope: two movable ones to measure between, plus fixed named
//! marks.
//!
//! No egui here, so the rules can be tested.

use studio_core::SimTime;

/// A named mark at a fixed time, e.g. "reset done".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub name: String,
    pub time: SimTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursors {
    /// Placed by clicking; the values column shows values here.
    pub primary: Option<SimTime>,
    /// Placed by Shift+clicking, for measuring from the primary cursor.
    pub secondary: Option<SimTime>,
    pub markers: Vec<Marker>,
    /// For naming new markers M1, M2, ... without repeating a name even
    /// after markers are deleted.
    markers_made: u32,
}

impl Cursors {
    /// The time between the two cursors, if both are placed.
    pub fn delta(&self) -> Option<SimTime> {
        let (a, b) = (self.primary?, self.secondary?);
        Some(SimTime::from_femtoseconds(a.femtoseconds.abs_diff(b.femtoseconds)))
    }

    /// Adds a marker at `time` named M1, M2, ... Returns its index.
    pub fn add_marker(&mut self, time: SimTime) -> usize {
        self.markers_made += 1;
        self.markers.push(Marker {
            name: format!("M{}", self.markers_made),
            time,
        });
        self.markers.len() - 1
    }

    pub fn remove_marker(&mut self, index: usize) {
        if index < self.markers.len() {
            self.markers.remove(index);
        }
    }
}

/// Where a click at `clicked` puts a cursor: on the nearest change of the
/// clicked signal (`before` / `after`) if one is within `tolerance`,
/// otherwise where clicked. This makes edge-to-edge measurements exact.
pub fn snap(clicked: SimTime, before: Option<SimTime>, after: Option<SimTime>, tolerance: SimTime) -> SimTime {
    let distance = |edge: SimTime| edge.femtoseconds.abs_diff(clicked.femtoseconds);
    [before, after]
        .into_iter()
        .flatten()
        .filter(|&edge| distance(edge) <= tolerance.femtoseconds)
        .min_by_key(|&edge| distance(edge))
        .unwrap_or(clicked)
}

/// `1 / period` as a frequency, e.g. a 10 ns period is `100 MHz`. `None`
/// for a zero period.
pub fn frequency_text(period: SimTime) -> Option<String> {
    if period.femtoseconds == 0 {
        return None;
    }
    let hertz = 1e15 / period.femtoseconds as f64;
    let (value, unit) = [(1e9, "GHz"), (1e6, "MHz"), (1e3, "kHz")]
        .into_iter()
        .find(|&(size, _)| hertz >= size)
        .map_or((hertz, "Hz"), |(size, unit)| (hertz / size, unit));
    // Up to four significant digits, without trailing zeros: 100 MHz, 13.33 MHz.
    let decimals = if value >= 100.0 { 1 } else if value >= 10.0 { 2 } else { 3 };
    let text = format!("{value:.decimals$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    Some(format!("{text} {unit}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(value: u64) -> SimTime {
        SimTime::from_femtoseconds(value * 1_000_000)
    }

    #[test]
    fn delta_needs_both_cursors_and_ignores_order() {
        let mut cursors = Cursors::default();
        assert_eq!(cursors.delta(), None);
        cursors.primary = Some(ns(50));
        assert_eq!(cursors.delta(), None);
        cursors.secondary = Some(ns(20));
        assert_eq!(cursors.delta(), Some(ns(30)));
        cursors.secondary = Some(ns(80));
        assert_eq!(cursors.delta(), Some(ns(30)));
    }

    #[test]
    fn markers_get_new_names_even_after_deleting() {
        let mut cursors = Cursors::default();
        cursors.add_marker(ns(10));
        cursors.add_marker(ns(20));
        cursors.remove_marker(0);
        let index = cursors.add_marker(ns(30));
        let names: Vec<&str> = cursors.markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["M2", "M3"]);
        assert_eq!(cursors.markers[index].time, ns(30));
        cursors.remove_marker(99); // out of range: ignored
        assert_eq!(cursors.markers.len(), 2);
    }

    #[test]
    fn snaps_to_the_nearest_close_edge() {
        let tolerance = ns(2);
        assert_eq!(snap(ns(101), Some(ns(100)), Some(ns(110)), tolerance), ns(100));
        assert_eq!(snap(ns(109), Some(ns(100)), Some(ns(110)), tolerance), ns(110));
        // Both edges too far: stays where clicked.
        assert_eq!(snap(ns(105), Some(ns(100)), Some(ns(110)), tolerance), ns(105));
        assert_eq!(snap(ns(105), None, None, tolerance), ns(105));
    }

    #[test]
    fn frequencies() {
        assert_eq!(frequency_text(ns(10)).as_deref(), Some("100 MHz"));
        assert_eq!(frequency_text(ns(8)).as_deref(), Some("125 MHz"));
        assert_eq!(frequency_text(ns(75)).as_deref(), Some("13.33 MHz"));
        assert_eq!(frequency_text(SimTime::from_femtoseconds(400_000)).as_deref(), Some("2.5 GHz"));
        // 115200 baud: one bit is about 8680.6 ns.
        assert_eq!(frequency_text(ns(8681)).as_deref(), Some("115.2 kHz"));
        assert_eq!(frequency_text(SimTime::parse("1 sec").unwrap()).as_deref(), Some("1 Hz"));
        assert_eq!(frequency_text(SimTime::ZERO), None);
    }
}
