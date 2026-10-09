//! Simulation time.

use std::fmt;

/// A point in simulation time, stored as a whole number of femtoseconds.
///
/// This matches VHDL, where `time` is an integer count of the base unit
/// `fs`. A `u64` holds about 5 hours of femtoseconds, which is far beyond
/// any real simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SimTime {
    pub femtoseconds: u64,
}

/// VHDL time units with their size in femtoseconds, largest first.
const UNITS: [(&str, u64); 8] = [
    ("hr", 3_600_000_000_000_000_000),
    ("min", 60_000_000_000_000_000),
    ("sec", 1_000_000_000_000_000),
    ("ms", 1_000_000_000_000),
    ("us", 1_000_000_000),
    ("ns", 1_000_000),
    ("ps", 1_000),
    ("fs", 1),
];

impl SimTime {
    pub const ZERO: SimTime = SimTime { femtoseconds: 0 };

    pub fn from_femtoseconds(femtoseconds: u64) -> SimTime {
        SimTime { femtoseconds }
    }

    /// Reads a time such as `"15ns"`, `"15 ns"` or `"15001ps"`.
    ///
    /// Only whole numbers are accepted, as NVC prints them. Returns `None`
    /// for anything else, including values too large to store.
    pub fn parse(text: &str) -> Option<SimTime> {
        let text = text.trim();
        let digits_end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(digits_end);
        let number: u64 = number.parse().ok()?;
        let unit = unit.trim();

        let (_, unit_size) = UNITS.iter().find(|(name, _)| *name == unit)?;
        let femtoseconds = number.checked_mul(*unit_size)?;
        Some(SimTime { femtoseconds })
    }

    /// The time in the largest unit that represents it exactly, as a
    /// number and unit name. For example 15 001 000 fs is `(15001, "ps")`.
    /// Zero is `(0, "fs")`.
    pub fn in_best_unit(self) -> (u64, &'static str) {
        if self.femtoseconds == 0 {
            return (0, "fs");
        }
        for (name, size) in UNITS {
            if self.femtoseconds.is_multiple_of(size) {
                return (self.femtoseconds / size, name);
            }
        }
        unreachable!("every time is a whole number of fs")
    }

    /// The time written the way NVC accepts it on the command line,
    /// e.g. `"15001ps"`.
    pub fn to_nvc_string(self) -> String {
        let (number, unit) = self.in_best_unit();
        format!("{number}{unit}")
    }
}

impl fmt::Display for SimTime {
    /// Writes e.g. `15001 ps`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (number, unit) = self.in_best_unit();
        write!(f, "{number} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs(femtoseconds: u64) -> SimTime {
        SimTime::from_femtoseconds(femtoseconds)
    }

    #[test]
    fn parses_units_as_nvc_prints_them() {
        assert_eq!(SimTime::parse("0ms"), Some(SimTime::ZERO));
        assert_eq!(SimTime::parse("10ns"), Some(fs(10_000_000)));
        assert_eq!(SimTime::parse("15001ps"), Some(fs(15_001_000)));
        assert_eq!(SimTime::parse("3015001ps"), Some(fs(3_015_001_000)));
    }

    #[test]
    fn parses_every_vhdl_unit() {
        assert_eq!(SimTime::parse("1fs"), Some(fs(1)));
        assert_eq!(SimTime::parse("1us"), Some(fs(1_000_000_000)));
        assert_eq!(SimTime::parse("2sec"), Some(fs(2_000_000_000_000_000)));
        assert_eq!(SimTime::parse("1min"), Some(fs(60_000_000_000_000_000)));
        assert_eq!(SimTime::parse("1hr"), Some(fs(3_600_000_000_000_000_000)));
    }

    #[test]
    fn allows_a_space_before_the_unit() {
        assert_eq!(SimTime::parse("5 ns"), Some(fs(5_000_000)));
        assert_eq!(SimTime::parse("  5 ns  "), Some(fs(5_000_000)));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(SimTime::parse(""), None);
        assert_eq!(SimTime::parse("ns"), None);
        assert_eq!(SimTime::parse("5"), None);
        assert_eq!(SimTime::parse("5 parsecs"), None);
        assert_eq!(SimTime::parse("1.5ns"), None);
        assert_eq!(SimTime::parse("-5ns"), None);
    }

    #[test]
    fn rejects_values_too_large_to_store() {
        assert_eq!(SimTime::parse("99999999hr"), None);
    }

    #[test]
    fn picks_the_largest_exact_unit() {
        assert_eq!(fs(15_001_000).in_best_unit(), (15001, "ps"));
        assert_eq!(fs(10_000_000).in_best_unit(), (10, "ns"));
        assert_eq!(fs(1_000_000_000).in_best_unit(), (1, "us"));
        assert_eq!(fs(7).in_best_unit(), (7, "fs"));
        assert_eq!(SimTime::ZERO.in_best_unit(), (0, "fs"));
    }

    #[test]
    fn formats_for_display_and_for_nvc() {
        assert_eq!(fs(15_001_000).to_string(), "15001 ps");
        assert_eq!(fs(1_000_000_000).to_nvc_string(), "1us");
    }

    #[test]
    fn round_trips_through_nvc_string() {
        for femtoseconds in [0, 1, 999, 1_000, 15_001_000, 3_600_000_000_000_000_000] {
            let time = fs(femtoseconds);
            assert_eq!(SimTime::parse(&time.to_nvc_string()), Some(time));
        }
    }
}
