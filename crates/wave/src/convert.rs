//! Converting between `wellen`'s types and ours. Keeping this in one place
//! means nothing outside this crate ever sees a `wellen` type.

use studio_core::{SimTime, Value, format_vhdl_time, parse_logic_vector};
use wellen::{ScopeType, SignalValueRef, Timescale, TimescaleUnit};

/// What kind of scope a tree node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// An entity/architecture instance (or a Verilog module).
    Instance,
    Block,
    Process,
    Generate,
    /// A record signal: its fields are the signals inside.
    Record,
    /// An array of records or vectors (with `--dump-arrays`).
    Array,
    Package,
    Other,
}

impl ScopeKind {
    pub(crate) fn from_wellen(scope_type: ScopeType) -> ScopeKind {
        match scope_type {
            ScopeType::VhdlArchitecture | ScopeType::Module | ScopeType::Interface => ScopeKind::Instance,
            ScopeType::VhdlBlock | ScopeType::Begin => ScopeKind::Block,
            ScopeType::VhdlProcess => ScopeKind::Process,
            ScopeType::VhdlForGenerate
            | ScopeType::VhdlIfGenerate
            | ScopeType::VhdlGenerate
            | ScopeType::Generate => ScopeKind::Generate,
            ScopeType::VhdlRecord | ScopeType::Struct => ScopeKind::Record,
            ScopeType::VhdlArray | ScopeType::SvArray => ScopeKind::Array,
            ScopeType::VhdlPackage | ScopeType::Package => ScopeKind::Package,
            _ => ScopeKind::Other,
        }
    }
}

/// How many femtoseconds one tick of the file's timescale is. nvc always
/// writes 1 fs; other tools may use e.g. 1 ps or 10 ns. Units finer than a
/// femtosecond (which VHDL can't express) and unknown units count as 1 fs.
pub(crate) fn femtoseconds_per_tick(timescale: Option<Timescale>) -> u64 {
    let Some(timescale) = timescale else {
        return 1;
    };
    let unit: u64 = match timescale.unit {
        TimescaleUnit::FemtoSeconds => 1,
        TimescaleUnit::PicoSeconds => 1_000,
        TimescaleUnit::NanoSeconds => 1_000_000,
        TimescaleUnit::MicroSeconds => 1_000_000_000,
        TimescaleUnit::MilliSeconds => 1_000_000_000_000,
        TimescaleUnit::Seconds => 1_000_000_000_000_000,
        TimescaleUnit::ZeptoSeconds | TimescaleUnit::AttoSeconds | TimescaleUnit::Unknown => 1,
    };
    (u64::from(timescale.factor) * unit).max(1)
}

/// Converts between file ticks and [`SimTime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TickScale {
    pub femtoseconds_per_tick: u64,
}

impl TickScale {
    pub fn to_time(self, ticks: u64) -> SimTime {
        SimTime::from_femtoseconds(ticks.saturating_mul(self.femtoseconds_per_tick))
    }
}

/// Converts one value. `is_vhdl_time` tidies nvc's text for `time`
/// signals (`"0 HR"` becomes `0 fs`).
pub(crate) fn to_value(value: SignalValueRef<'_>, is_vhdl_time: bool) -> Value {
    match value {
        SignalValueRef::BitVec(bits) => {
            let text = bits.bit_string();
            match parse_logic_vector(&text) {
                Some(logic) => Value::Logic(logic),
                // Not expected from nvc, but never lose information.
                None => Value::Text(text),
            }
        }
        SignalValueRef::String(text) if is_vhdl_time => Value::Text(format_vhdl_time(text)),
        SignalValueRef::String(text) => Value::Text(text.to_string()),
        SignalValueRef::Real(number) => Value::Real(number),
        // Verilog events; VHDL has nothing like them.
        SignalValueRef::Event => Value::Text("event".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::Logic;
    use wellen::States;

    #[test]
    fn timescales_in_femtoseconds() {
        let scale = |factor, unit| femtoseconds_per_tick(Some(Timescale { factor, unit }));
        assert_eq!(scale(1, TimescaleUnit::FemtoSeconds), 1);
        assert_eq!(scale(1, TimescaleUnit::PicoSeconds), 1_000);
        assert_eq!(scale(10, TimescaleUnit::NanoSeconds), 10_000_000);
        assert_eq!(scale(1, TimescaleUnit::Unknown), 1);
        assert_eq!(femtoseconds_per_tick(None), 1);
    }

    #[test]
    fn ticks_and_times() {
        let ps = TickScale {
            femtoseconds_per_tick: 1_000,
        };
        assert_eq!(ps.to_time(15), SimTime::parse("15 ps").unwrap());
    }

    #[test]
    fn converts_each_kind_of_value() {
        // A 4-bit two-state value, packed the way wellen stores it.
        let two_state = SignalValueRef::bit_vec(States::Two, 4, &[0b1010]);
        assert_eq!(
            to_value(two_state, false),
            Value::Logic(vec![Logic::One, Logic::Zero, Logic::One, Logic::Zero])
        );
        assert_eq!(to_value(SignalValueRef::String("idle"), false), Value::Text("idle".to_string()));
        assert_eq!(to_value(SignalValueRef::String("0 HR"), true), Value::Text("0 fs".to_string()));
        assert_eq!(to_value(SignalValueRef::Real(3.25), false), Value::Real(3.25));
    }
}
