//! Signal values: std_logic's nine values, and what a signal holds at one
//! moment in a waveform.

use std::fmt;

/// One std_logic value. The order follows the VHDL declaration of
/// `std_ulogic`: `'U', 'X', '0', '1', 'Z', 'W', 'L', 'H', '-'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Logic {
    /// Uninitialised.
    U,
    /// Forcing unknown (e.g. two drivers fighting).
    X,
    Zero,
    One,
    /// High impedance.
    Z,
    /// Weak unknown.
    W,
    /// Weak 0 (e.g. a pull-down).
    L,
    /// Weak 1 (e.g. a pull-up).
    H,
    /// Don't care.
    DontCare,
}

impl Logic {
    /// Reads a std_logic character, either case: `'u'` and `'U'` are both U.
    pub fn from_char(c: char) -> Option<Logic> {
        Some(match c.to_ascii_uppercase() {
            'U' => Logic::U,
            'X' => Logic::X,
            '0' => Logic::Zero,
            '1' => Logic::One,
            'Z' => Logic::Z,
            'W' => Logic::W,
            'L' => Logic::L,
            'H' => Logic::H,
            '-' => Logic::DontCare,
            _ => return None,
        })
    }

    /// The character as VHDL writes it, upper case: `'U'`, `'0'`, `'-'`...
    pub fn to_char(self) -> char {
        match self {
            Logic::U => 'U',
            Logic::X => 'X',
            Logic::Zero => '0',
            Logic::One => '1',
            Logic::Z => 'Z',
            Logic::W => 'W',
            Logic::L => 'L',
            Logic::H => 'H',
            Logic::DontCare => '-',
        }
    }

    /// The value as a number bit: `0` and `L` are 0, `1` and `H` are 1,
    /// anything else has no numeric value. Weak values count, as in
    /// `numeric_std`'s `to_01`.
    pub fn to_bit(self) -> Option<bool> {
        match self {
            Logic::Zero | Logic::L => Some(false),
            Logic::One | Logic::H => Some(true),
            _ => None,
        }
    }
}

impl fmt::Display for Logic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

/// Reads a string of std_logic characters such as `"01XZ"`. The first
/// character is the leftmost (most significant) bit.
pub fn parse_logic_vector(text: &str) -> Option<Vec<Logic>> {
    text.chars().map(Logic::from_char).collect()
}

/// What a signal holds at one moment.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// std_logic, std_logic_vector, signed, unsigned, bit, integer... The
    /// first element is the leftmost (most significant) bit, as written in
    /// VHDL: `"0011"` is 3.
    Logic(Vec<Logic>),
    /// Enumeration names (`idle`), booleans (`true`) and other values nvc
    /// writes as text.
    Text(String),
    Real(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Logic; 9] = [
        Logic::U,
        Logic::X,
        Logic::Zero,
        Logic::One,
        Logic::Z,
        Logic::W,
        Logic::L,
        Logic::H,
        Logic::DontCare,
    ];

    #[test]
    fn characters_round_trip_in_either_case() {
        for logic in ALL {
            let c = logic.to_char();
            assert_eq!(Logic::from_char(c), Some(logic));
            assert_eq!(Logic::from_char(c.to_ascii_lowercase()), Some(logic));
        }
        assert_eq!(Logic::from_char('2'), None);
    }

    #[test]
    fn weak_values_count_as_numbers() {
        assert_eq!(Logic::L.to_bit(), Some(false));
        assert_eq!(Logic::H.to_bit(), Some(true));
        assert_eq!(Logic::Zero.to_bit(), Some(false));
        assert_eq!(Logic::One.to_bit(), Some(true));
        for unknown in [Logic::U, Logic::X, Logic::Z, Logic::W, Logic::DontCare] {
            assert_eq!(unknown.to_bit(), None, "{unknown}");
        }
    }

    #[test]
    fn parses_vectors_leftmost_first() {
        assert_eq!(
            parse_logic_vector("01xZ"),
            Some(vec![Logic::Zero, Logic::One, Logic::X, Logic::Z])
        );
        assert_eq!(parse_logic_vector("012"), None);
        assert_eq!(parse_logic_vector(""), Some(vec![]));
    }
}
