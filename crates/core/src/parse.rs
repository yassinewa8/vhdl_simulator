//! Reading a value the user typed (for "find next time the bus is A5"),
//! the reverse of `radix.rs`.

use crate::radix::Radix;
use crate::value::Logic;

/// Reads `text` as a `width`-bit value, leftmost bit first.
///
/// Bare digits are read in `radix`, the radix the signal is shown in, so
/// `A5` on a hex bus is hex and `-3` on a signed one is -3. A prefix
/// overrides it: `0x1F`, `x"1F"` or `16#1F#` (hex), `0b0101` or `b"0101"`
/// (binary), `0o17` or `8#17#` (octal). A single letter `U`, `X`, `Z`,
/// `W`, `L`, `H` or `-` means every bit is that value. Hex and binary
/// digits may also be std_logic letters (`0x5X`, `01XZ`). Underscores are
/// ignored (`1_000`).
pub fn parse_logic_value(text: &str, radix: Radix, width: usize) -> Result<Vec<Logic>, String> {
    let text: String = text.trim().chars().filter(|&c| c != '_').collect();
    if text.is_empty() {
        return Err("type a value".to_string());
    }

    // One std_logic letter (not the digits 0 and 1): every bit the same.
    let mut letters = text.chars();
    if let (Some(only), None) = (letters.next(), letters.next())
        && !matches!(only, '0' | '1')
        && let Some(logic) = Logic::from_char(only)
    {
        return Ok(vec![logic; width]);
    }

    let lower = text.to_lowercase();
    let bits = if let Some(digits) = strip_prefix(&lower, &["0x"], &[("x\"", "\""), ("16#", "#")]) {
        digits_to_bits(digits, 4)?
    } else if let Some(digits) = strip_prefix(&lower, &["0b"], &[("b\"", "\""), ("2#", "#")]) {
        digits_to_bits(digits, 1)?
    } else if let Some(digits) = strip_prefix(&lower, &["0o"], &[("o\"", "\""), ("8#", "#")]) {
        digits_to_bits(digits, 3)?
    } else {
        match radix {
            Radix::Binary => digits_to_bits(&lower, 1)?,
            Radix::Hex => digits_to_bits(&lower, 4)?,
            Radix::Octal => digits_to_bits(&lower, 3)?,
            Radix::Unsigned | Radix::Signed => return decimal_to_bits(&lower, radix, width),
            Radix::Ascii => text.bytes().flat_map(|byte| (0..8).rev().map(move |bit| bit_of(byte >> bit & 1 == 1))).collect(),
        }
    };
    fit_to_width(bits, width)
}

/// `text` without a leading prefix (`0x`) or surrounding pair (`x"…"`).
fn strip_prefix<'a>(text: &'a str, prefixes: &[&str], pairs: &[(&str, &str)]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
        .or_else(|| pairs.iter().find_map(|(open, close)| text.strip_prefix(open)?.strip_suffix(close)))
}

fn bit_of(one: bool) -> Logic {
    if one { Logic::One } else { Logic::Zero }
}

/// Each digit becomes `bits_per_digit` bits; a std_logic letter becomes
/// that many copies of itself (so hex `X` is `XXXX`).
fn digits_to_bits(digits: &str, bits_per_digit: u32) -> Result<Vec<Logic>, String> {
    let radix = 1 << bits_per_digit;
    let mut bits = Vec::new();
    for c in digits.chars() {
        if let Some(number) = c.to_digit(radix) {
            bits.extend((0..bits_per_digit).rev().map(|bit| bit_of(number >> bit & 1 == 1)));
        } else if let Some(logic) = Logic::from_char(c) {
            bits.extend(std::iter::repeat_n(logic, bits_per_digit as usize));
        } else {
            return Err(format!("'{c}' is not a base-{radix} digit"));
        }
    }
    if bits.is_empty() { Err("no digits".to_string()) } else { Ok(bits) }
}

/// Pads with zeros on the left, or drops leading zeros, to `width` bits.
fn fit_to_width(mut bits: Vec<Logic>, width: usize) -> Result<Vec<Logic>, String> {
    if bits.len() > width {
        let extra = bits.len() - width;
        if bits[..extra].iter().any(|&bit| bit != Logic::Zero) {
            return Err(format!("too big for {width} bits"));
        }
        bits.drain(..extra);
    }
    let mut padded = vec![Logic::Zero; width - bits.len()];
    padded.extend(bits);
    Ok(padded)
}

/// A decimal number as `width` bits, two's complement when negative.
fn decimal_to_bits(text: &str, radix: Radix, width: usize) -> Result<Vec<Logic>, String> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let magnitude: u128 = digits.parse().map_err(|_| format!("'{text}' is not a decimal number"))?;
    // N bits hold 0 to 2^N - 1 unsigned, or -2^(N-1) to 2^(N-1) - 1 signed.
    let fits = if negative {
        magnitude == 0 || below_power_of_two(magnitude - 1, width.saturating_sub(1))
    } else if radix == Radix::Signed {
        below_power_of_two(magnitude, width.saturating_sub(1))
    } else {
        below_power_of_two(magnitude, width)
    };
    if !fits {
        return Err(format!("{text} doesn't fit in {width} bits"));
    }
    let mut bits: Vec<bool> = (0..width).rev().map(|bit| bit < 128 && magnitude >> bit & 1 == 1).collect();
    if negative {
        for bit in &mut bits {
            *bit = !*bit;
        }
        // Add one, from the right.
        for bit in bits.iter_mut().rev() {
            *bit = !*bit;
            if *bit {
                break;
            }
        }
    }
    Ok(bits.into_iter().map(bit_of).collect())
}

/// True if `number` is less than 2 to the power `bits`.
fn below_power_of_two(number: u128, bits: usize) -> bool {
    bits >= 128 || number < 1u128 << bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radix::format_logic;
    use crate::value::parse_logic_vector;

    fn parse(text: &str, radix: Radix, width: usize) -> String {
        match parse_logic_value(text, radix, width) {
            Ok(bits) => format_logic(&bits, Radix::Binary),
            Err(error) => format!("error: {error}"),
        }
    }

    #[test]
    fn bare_digits_use_the_signals_radix() {
        assert_eq!(parse("A5", Radix::Hex, 8), "10100101");
        assert_eq!(parse("10", Radix::Hex, 8), "00010000");
        assert_eq!(parse("10", Radix::Unsigned, 8), "00001010");
        assert_eq!(parse("101", Radix::Binary, 4), "0101");
        assert_eq!(parse("17", Radix::Octal, 6), "001111");
        assert_eq!(parse("Hi", Radix::Ascii, 16), "0100100001101001");
    }

    #[test]
    fn prefixes_override_the_radix() {
        for text in ["0x1F", "x\"1F\"", "16#1F#", "0X1f"] {
            assert_eq!(parse(text, Radix::Unsigned, 8), "00011111", "{text}");
        }
        for text in ["0b0101", "b\"0101\"", "2#0101#"] {
            assert_eq!(parse(text, Radix::Hex, 4), "0101", "{text}");
        }
        assert_eq!(parse("0o17", Radix::Hex, 6), "001111");
        assert_eq!(parse("1_000", Radix::Unsigned, 16), "0000001111101000");
    }

    #[test]
    fn std_logic_letters() {
        assert_eq!(parse("X", Radix::Hex, 4), "XXXX");
        assert_eq!(parse("u", Radix::Signed, 3), "UUU");
        assert_eq!(parse("Z", Radix::Binary, 2), "ZZ");
        assert_eq!(parse("0x5X", Radix::Hex, 8), "0101XXXX");
        assert_eq!(parse("01XZ", Radix::Binary, 4), "01XZ");
        // 0 and 1 on their own are numbers, not "every bit".
        assert_eq!(parse("1", Radix::Binary, 4), "0001");
    }

    #[test]
    fn decimals_signed_and_unsigned() {
        assert_eq!(parse("-3", Radix::Signed, 8), "11111101");
        assert_eq!(parse("-128", Radix::Signed, 8), "10000000");
        assert_eq!(parse("127", Radix::Signed, 8), "01111111");
        assert_eq!(parse("255", Radix::Unsigned, 8), "11111111");
        assert_eq!(parse("-1", Radix::Unsigned, 4), "1111", "a minus sign means two's complement");
        assert!(parse("128", Radix::Signed, 8).starts_with("error"));
        assert!(parse("-129", Radix::Signed, 8).starts_with("error"));
        assert!(parse("256", Radix::Unsigned, 8).starts_with("error"));
        assert!(parse("12a", Radix::Unsigned, 8).starts_with("error"));
    }

    #[test]
    fn widths_and_errors() {
        assert_eq!(parse("0x0F", Radix::Hex, 4), "1111", "leading zeros dropped");
        assert_eq!(parse("0x1F", Radix::Hex, 4), "error: too big for 4 bits");
        assert_eq!(parse("G", Radix::Hex, 8), "error: 'g' is not a base-16 digit");
        assert_eq!(parse("  ", Radix::Hex, 8), "error: type a value");
        // A wide bus in decimal.
        let bits = parse_logic_value("1", Radix::Unsigned, 200).unwrap();
        assert_eq!(bits.len(), 200);
        assert_eq!(bits[199], Logic::One);
        assert_eq!(parse_logic_value("-1", Radix::Signed, 200).unwrap(), vec![Logic::One; 200]);
    }

    #[test]
    fn round_trips_with_the_formatter() {
        for (text, radix) in [("A5", Radix::Hex), ("-3", Radix::Signed), ("200", Radix::Unsigned), ("245", Radix::Octal)] {
            let bits = parse_logic_value(text, radix, 8).unwrap();
            assert_eq!(format_logic(&bits, radix), text);
        }
        assert_eq!(parse_logic_value("01XZ", Radix::Binary, 4).unwrap(), parse_logic_vector("01XZ").unwrap());
    }
}
