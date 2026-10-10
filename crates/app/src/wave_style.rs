//! How a value looks in the wave view: which colour, and whether a single
//! bit is drawn as a line or a block.
//!
//! No drawing here, only the rules, so they can be tested. The colours
//! sit in one `Palette` so they can be made configurable later.

use eframe::egui::Color32;
use studio_core::{Logic, Value};

/// The kind of value, for choosing a colour and shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// '0' and '1', and any known text or real value.
    Strong,
    /// 'L' and 'H': a pull-down or pull-up. Same height as 0/1, dashed.
    Weak,
    /// 'Z': nothing driving.
    HighZ,
    /// 'X': forcing unknown, e.g. two drivers fighting.
    Unknown,
    /// 'U': never assigned.
    Uninitialised,
    /// 'W': weak unknown.
    WeakUnknown,
    /// '-': don't care.
    DontCare,
}

impl Look {
    /// The look of a single std_logic value.
    fn of_logic(bit: Logic) -> Look {
        match bit {
            Logic::Zero | Logic::One => Look::Strong,
            Logic::L | Logic::H => Look::Weak,
            Logic::Z => Look::HighZ,
            Logic::X => Look::Unknown,
            Logic::U => Look::Uninitialised,
            Logic::W => Look::WeakUnknown,
            Logic::DontCare => Look::DontCare,
        }
    }

    /// Which look wins when a bus mixes several: the most alarming one,
    /// so a single X bit makes the whole bus red.
    fn priority(self) -> u8 {
        match self {
            Look::Unknown => 6,
            Look::WeakUnknown => 5,
            Look::DontCare => 4,
            Look::Uninitialised => 3,
            Look::HighZ => 2,
            Look::Weak => 1,
            Look::Strong => 0,
        }
    }

    /// True for values drawn as a filled block on a one-bit signal (they
    /// have no sensible high or low level).
    pub fn is_block(self) -> bool {
        matches!(self, Look::Unknown | Look::Uninitialised | Look::WeakUnknown | Look::DontCare)
    }
}

/// How a whole value (one bit or a bus) is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub look: Look,
    /// Every bit is 'Z': a bus is then drawn as a mid-level line like a
    /// single Z bit, the usual way to show a released bus.
    pub all_high_z: bool,
}

/// The style of a value. Text (enums, booleans) and reals are strong.
pub fn style_of(value: &Value) -> Style {
    let Value::Logic(bits) = value else {
        return Style {
            look: Look::Strong,
            all_high_z: false,
        };
    };
    let look = bits
        .iter()
        .map(|&bit| Look::of_logic(bit))
        .max_by_key(|look| look.priority())
        .unwrap_or(Look::Strong);
    Style {
        look,
        all_high_z: !bits.is_empty() && bits.iter().all(|&bit| bit == Logic::Z),
    }
}

/// Where a one-bit value sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    High,
    Low,
    Middle,
    /// Fills the row from top to bottom.
    Block,
}

/// 1 and H high, 0 and L low, Z in the middle, the unknowns as a block.
pub fn level_of(value: &Value) -> Level {
    let bit = match value {
        Value::Logic(bits) if bits.len() == 1 => bits[0],
        _ => return Level::Middle,
    };
    match bit {
        Logic::One | Logic::H => Level::High,
        Logic::Zero | Logic::L => Level::Low,
        Logic::Z => Level::Middle,
        Logic::X | Logic::U | Logic::W | Logic::DontCare => Level::Block,
    }
}

/// The colours of the wave view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub strong: Color32,
    pub weak: Color32,
    pub high_z: Color32,
    pub unknown: Color32,
    pub uninitialised: Color32,
    pub weak_unknown: Color32,
    pub dont_care: Color32,
    /// Bus values printed inside a known bus.
    pub text: Color32,
}

impl Palette {
    /// Colours chosen for a dark background, in the spirit of classic
    /// simulator wave windows: green for good values, red for X.
    pub fn dark() -> Palette {
        Palette {
            strong: Color32::from_rgb(0x4c, 0xd9, 0x64),
            weak: Color32::from_rgb(0x8f, 0xd9, 0xb6),
            high_z: Color32::from_rgb(0xe6, 0xc8, 0x3c),
            unknown: Color32::from_rgb(0xf0, 0x4a, 0x4a),
            uninitialised: Color32::from_rgb(0xf0, 0x8c, 0x32),
            weak_unknown: Color32::from_rgb(0xc0, 0x6a, 0xe6),
            dont_care: Color32::from_rgb(0x7a, 0x9c, 0xc8),
            text: Color32::from_rgb(0xf0, 0xf0, 0xf0),
        }
    }

    /// Darker versions of the same colours, readable on white.
    pub fn light() -> Palette {
        Palette {
            strong: Color32::from_rgb(0x1a, 0x8a, 0x2e),
            weak: Color32::from_rgb(0x2e, 0x8f, 0x7a),
            high_z: Color32::from_rgb(0xa8, 0x86, 0x00),
            unknown: Color32::from_rgb(0xc8, 0x1e, 0x1e),
            uninitialised: Color32::from_rgb(0xc0, 0x60, 0x00),
            weak_unknown: Color32::from_rgb(0x8a, 0x3c, 0xb4),
            dont_care: Color32::from_rgb(0x46, 0x64, 0x96),
            text: Color32::from_rgb(0x10, 0x10, 0x10),
        }
    }

    pub fn for_theme(dark_mode: bool) -> Palette {
        if dark_mode { Palette::dark() } else { Palette::light() }
    }

    pub fn colour(&self, look: Look) -> Color32 {
        match look {
            Look::Strong => self.strong,
            Look::Weak => self.weak,
            Look::HighZ => self.high_z,
            Look::Unknown => self.unknown,
            Look::Uninitialised => self.uninitialised,
            Look::WeakUnknown => self.weak_unknown,
            Look::DontCare => self.dont_care,
        }
    }

    /// The colour for printing a value: the normal text colour when it's
    /// known, otherwise the colour of what's wrong with it.
    pub fn text_colour(&self, look: Look) -> Color32 {
        match look {
            Look::Strong | Look::Weak => self.text,
            other => self.colour(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::parse_logic_vector;

    fn logic(text: &str) -> Value {
        Value::Logic(parse_logic_vector(text).unwrap())
    }

    #[test]
    fn single_bits() {
        let cases = [
            ("0", Look::Strong, Level::Low),
            ("1", Look::Strong, Level::High),
            ("L", Look::Weak, Level::Low),
            ("H", Look::Weak, Level::High),
            ("Z", Look::HighZ, Level::Middle),
            ("X", Look::Unknown, Level::Block),
            ("U", Look::Uninitialised, Level::Block),
            ("W", Look::WeakUnknown, Level::Block),
            ("-", Look::DontCare, Level::Block),
        ];
        for (bit, look, level) in cases {
            assert_eq!(style_of(&logic(bit)).look, look, "{bit}");
            assert_eq!(level_of(&logic(bit)), level, "{bit}");
        }
    }

    #[test]
    fn a_bus_takes_its_most_alarming_bit() {
        assert_eq!(style_of(&logic("10100101")).look, Look::Strong);
        assert_eq!(style_of(&logic("1010HHLL")).look, Look::Weak);
        assert_eq!(style_of(&logic("0Z10")).look, Look::HighZ);
        assert_eq!(style_of(&logic("UU10")).look, Look::Uninitialised);
        assert_eq!(style_of(&logic("U-10")).look, Look::DontCare);
        assert_eq!(style_of(&logic("01XZWLH-")).look, Look::Unknown, "one X makes it red");
    }

    #[test]
    fn only_a_fully_released_bus_is_drawn_mid_level() {
        assert!(style_of(&logic("ZZZZ")).all_high_z);
        assert!(!style_of(&logic("ZZZ0")).all_high_z);
        assert!(!style_of(&logic("")).all_high_z);
    }

    #[test]
    fn text_and_reals_are_strong() {
        assert_eq!(style_of(&Value::Text("idle".to_string())).look, Look::Strong);
        assert_eq!(style_of(&Value::Real(1.5)).look, Look::Strong);
        assert_eq!(level_of(&Value::Text("idle".to_string())), Level::Middle);
    }

    #[test]
    fn every_look_has_its_own_colour() {
        for palette in [Palette::dark(), Palette::light()] {
            let looks = [
                Look::Strong,
                Look::Weak,
                Look::HighZ,
                Look::Unknown,
                Look::Uninitialised,
                Look::WeakUnknown,
                Look::DontCare,
            ];
            let colours: Vec<Color32> = looks.iter().map(|&l| palette.colour(l)).collect();
            for (i, a) in colours.iter().enumerate() {
                for b in &colours[i + 1..] {
                    assert_ne!(a, b, "two looks share a colour");
                }
            }
        }
    }

    #[test]
    fn values_print_in_the_colour_of_whats_wrong() {
        let palette = Palette::dark();
        assert_eq!(palette.text_colour(Look::Strong), palette.text);
        assert_eq!(palette.text_colour(Look::Unknown), palette.unknown);
    }
}
