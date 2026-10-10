//! Turning signal values into text: binary, hex, decimal and so on.
//!
//! Vectors containing unknowns (U, X, Z, W, -) never show a made-up
//! number. A hex or octal digit whose bits are all the same unknown letter
//! shows that letter (`XXXX0101` is `X5`); a digit mixing unknowns with
//! other values shows `?`. Decimal works the same way for the whole value.
//! Weak values L and H count as 0 and 1, as in `numeric_std`.

use std::fmt;

use crate::time::SimTime;
use crate::value::{Logic, Value};

/// How to show a vector's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radix {
    Binary,
    Hex,
    Octal,
    Unsigned,
    Signed,
    /// Each 8 bits as a character, e.g. a string held in a vector.
    Ascii,
}

impl Radix {
    pub const ALL: [Radix; 6] = [
        Radix::Binary,
        Radix::Hex,
        Radix::Octal,
        Radix::Unsigned,
        Radix::Signed,
        Radix::Ascii,
    ];

    /// Name for menus.
    pub fn name(self) -> &'static str {
        match self {
            Radix::Binary => "Binary",
            Radix::Hex => "Hexadecimal",
            Radix::Octal => "Octal",
            Radix::Unsigned => "Unsigned decimal",
            Radix::Signed => "Signed decimal",
            Radix::Ascii => "ASCII",
        }
    }
}

impl fmt::Display for Radix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A radix that suits a signal of this VHDL type and width: single bits
/// in binary, `signed` and integer types as signed decimal, other vectors
/// in hex. `vhdl_type` is the type name as nvc records it (`SIGNED`).
pub fn default_radix(vhdl_type: Option<&str>, width: usize) -> Radix {
    if width <= 1 {
        return Radix::Binary;
    }
    let signed_types = ["SIGNED", "INTEGER", "NATURAL", "POSITIVE", "SFIXED"];
    match vhdl_type {
        Some(name) if signed_types.iter().any(|t| t.eq_ignore_ascii_case(name)) => Radix::Signed,
        _ => Radix::Hex,
    }
}

/// Formats any value. Text and reals ignore the radix.
pub fn format_value(value: &Value, radix: Radix) -> String {
    match value {
        Value::Logic(bits) => format_logic(bits, radix),
        Value::Text(text) => text.clone(),
        Value::Real(number) => number.to_string(),
    }
}

/// Formats a vector, leftmost (most significant) bit first.
pub fn format_logic(bits: &[Logic], radix: Radix) -> String {
    match radix {
        Radix::Binary => bits.iter().map(|bit| bit.to_char()).collect(),
        Radix::Hex => format_in_groups(bits, 4),
        Radix::Octal => format_in_groups(bits, 3),
        Radix::Unsigned => format_decimal(bits, false),
        Radix::Signed => format_decimal(bits, true),
        Radix::Ascii => format_ascii(bits),
    }
}

/// Hex (groups of 4) or octal (groups of 3), grouping from the right so
/// the leftmost digit may cover fewer bits.
fn format_in_groups(bits: &[Logic], group_size: usize) -> String {
    let mut digits: Vec<char> = bits.rchunks(group_size).map(group_digit).collect();
    digits.reverse();
    digits.into_iter().collect()
}

/// One hex/octal digit: its value if every bit is known, the letter if all
/// bits are the same unknown, otherwise `?`.
fn group_digit(group: &[Logic]) -> char {
    match known_number(group) {
        Some(number) => char::from_digit(number as u32, 16)
            .expect("a group of up to 4 bits is one digit")
            .to_ascii_uppercase(),
        None => uniform_letter(group).unwrap_or('?'),
    }
}

/// The value of a short vector, if every bit is 0/1/L/H.
fn known_number(bits: &[Logic]) -> Option<u64> {
    bits.iter()
        .try_fold(0u64, |number, bit| Some(number * 2 + u64::from(bit.to_bit()?)))
}

/// The letter of a vector whose bits are all the same unknown value.
fn uniform_letter(bits: &[Logic]) -> Option<char> {
    let first = *bits.first()?;
    let all_same = bits.iter().all(|&bit| bit == first);
    (all_same && first.to_bit().is_none()).then(|| first.to_char())
}

/// Decimal of any width (vectors can be wider than 128 bits).
fn format_decimal(bits: &[Logic], signed: bool) -> String {
    if bits.is_empty() {
        return String::new();
    }
    let Some(mut values) = bits.iter().map(|bit| bit.to_bit()).collect::<Option<Vec<bool>>>() else {
        return uniform_letter(bits).map(String::from).unwrap_or_else(|| "?".to_string());
    };

    // A negative two's complement number: show '-' and the magnitude,
    // which is "invert every bit, then add one".
    let negative = signed && values[0];
    if negative {
        for value in &mut values {
            *value = !*value;
        }
        add_one(&mut values);
    }
    let digits = binary_to_decimal(&values);
    if negative { format!("-{digits}") } else { digits }
}

/// Adds 1 to a binary number (leftmost bit most significant), in place.
fn add_one(bits: &mut [bool]) {
    for bit in bits.iter_mut().rev() {
        *bit = !*bit;
        if *bit {
            return; // that bit went 0 -> 1, so no carry
        }
    }
}

/// Converts binary to a decimal string by "double and add" on a list of
/// decimal digits, so it works for any width.
fn binary_to_decimal(bits: &[bool]) -> String {
    // Least significant digit first.
    let mut digits: Vec<u8> = vec![0];
    for &bit in bits {
        let mut carry = u8::from(bit);
        for digit in &mut digits {
            let doubled = *digit * 2 + carry;
            *digit = doubled % 10;
            carry = doubled / 10;
        }
        if carry > 0 {
            digits.push(carry);
        }
    }
    digits.iter().rev().map(|digit| char::from(b'0' + digit)).collect()
}

/// Each 8 bits (grouped from the right) as a character: printable ASCII
/// as itself, other bytes as `.`, bytes with unknown bits as `?`.
fn format_ascii(bits: &[Logic]) -> String {
    let mut text: Vec<char> = bits
        .rchunks(8)
        .map(|byte| match known_number(byte) {
            Some(code) if (0x20..0x7f).contains(&code) => char::from(code as u8),
            Some(_) => '.',
            None => '?',
        })
        .collect();
    text.reverse();
    text.into_iter().collect()
}

/// Tidies a VHDL `time` value as nvc writes it in waveforms (`"7 NS"`,
/// and `"0 HR"` for zero) into our usual form (`7 ns`, `0 fs`). Text that
/// isn't a time is returned unchanged.
pub fn format_vhdl_time(text: &str) -> String {
    match SimTime::parse(&text.to_lowercase()) {
        Some(time) => time.to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::parse_logic_vector;

    fn fmt(bits: &str, radix: Radix) -> String {
        format_logic(&parse_logic_vector(bits).unwrap(), radix)
    }

    #[test]
    fn binary_shows_every_value() {
        assert_eq!(fmt("01xzwlh-u", Radix::Binary), "01XZWLH-U");
    }

    #[test]
    fn hex_and_octal_group_from_the_right() {
        assert_eq!(fmt("10100101", Radix::Hex), "A5");
        assert_eq!(fmt("110100101", Radix::Hex), "1A5");
        assert_eq!(fmt("10100101", Radix::Octal), "245");
        assert_eq!(fmt("1", Radix::Hex), "1");
    }

    #[test]
    fn hex_with_unknowns_never_shows_a_wrong_digit() {
        assert_eq!(fmt("XXXX0101", Radix::Hex), "X5");
        assert_eq!(fmt("UUUUUUUU", Radix::Hex), "UU");
        assert_eq!(fmt("ZZZZ", Radix::Hex), "Z");
        assert_eq!(fmt("0X10", Radix::Hex), "?");
        assert_eq!(fmt("XXXZ", Radix::Hex), "?");
        // Weak values are numbers: LHHL is 0110.
        assert_eq!(fmt("LHHL", Radix::Hex), "6");
    }

    #[test]
    fn decimal_unsigned_and_signed() {
        assert_eq!(fmt("00101010", Radix::Unsigned), "42");
        assert_eq!(fmt("11111101", Radix::Unsigned), "253");
        assert_eq!(fmt("11111101", Radix::Signed), "-3");
        assert_eq!(fmt("10000000", Radix::Signed), "-128");
        assert_eq!(fmt("01111111", Radix::Signed), "127");
        assert_eq!(fmt("1", Radix::Signed), "-1");
        assert_eq!(fmt("0", Radix::Signed), "0");
    }

    #[test]
    fn decimal_works_beyond_128_bits() {
        let mut bits = "1".to_string();
        bits.push_str(&"0".repeat(130)); // 2^130
        assert_eq!(fmt(&bits, Radix::Unsigned), "1361129467683753853853498429727072845824");
        // All ones is -1 when signed, whatever the width.
        assert_eq!(fmt(&"1".repeat(200), Radix::Signed), "-1");
    }

    #[test]
    fn decimal_with_unknowns() {
        assert_eq!(fmt("UUUU", Radix::Unsigned), "U");
        assert_eq!(fmt("XXXX", Radix::Signed), "X");
        assert_eq!(fmt("01X1", Radix::Unsigned), "?");
    }

    #[test]
    fn ascii_text_in_a_vector() {
        assert_eq!(fmt("0100100001101001", Radix::Ascii), "Hi");
        assert_eq!(fmt("00000000", Radix::Ascii), ".");
        assert_eq!(fmt("0100100X", Radix::Ascii), "?");
    }

    #[test]
    fn empty_vector_is_empty_text() {
        for radix in Radix::ALL {
            assert_eq!(format_logic(&[], radix), "", "{radix}");
        }
    }

    #[test]
    fn default_radix_follows_the_vhdl_type() {
        assert_eq!(default_radix(Some("STD_LOGIC"), 1), Radix::Binary);
        assert_eq!(default_radix(Some("SIGNED"), 8), Radix::Signed);
        assert_eq!(default_radix(Some("INTEGER"), 32), Radix::Signed);
        assert_eq!(default_radix(Some("UNSIGNED"), 8), Radix::Hex);
        assert_eq!(default_radix(Some("STD_LOGIC_VECTOR"), 8), Radix::Hex);
        assert_eq!(default_radix(None, 16), Radix::Hex);
    }

    #[test]
    fn text_and_real_values_ignore_the_radix() {
        assert_eq!(format_value(&Value::Text("idle".to_string()), Radix::Hex), "idle");
        assert_eq!(format_value(&Value::Real(3.25), Radix::Binary), "3.25");
        let bits = Value::Logic(parse_logic_vector("1010").unwrap());
        assert_eq!(format_value(&bits, Radix::Hex), "A");
    }

    #[test]
    fn vhdl_time_text_is_tidied() {
        assert_eq!(format_vhdl_time("0 HR"), "0 fs");
        assert_eq!(format_vhdl_time("7 NS"), "7 ns");
        assert_eq!(format_vhdl_time("1500 PS"), "1500 ps");
        assert_eq!(format_vhdl_time("idle"), "idle");
    }
}
