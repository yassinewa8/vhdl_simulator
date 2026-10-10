//! Shared types used by every other crate in the workspace.
//!
//! - [`Severity`]: note, warning, error, failure, fatal
//! - [`SimTime`]: simulation time in femtoseconds
//! - [`Diagnostic`], [`SimMessage`], [`Location`]: structured messages from
//!   the compiler and the simulator
//! - [`Logic`], [`Value`]: std_logic's nine values and signal values
//! - [`Radix`], [`format_value`]: showing values as binary, hex, decimal...
//! - [`parse_logic_value`]: reading a value the user typed
//! - [`CancelToken`]: the Stop flag for background jobs
//!
//! Nothing here may depend on the GUI.

mod cancel;
mod diagnostic;
mod parse;
mod radix;
mod severity;
mod time;
mod value;

pub use cancel::CancelToken;
pub use diagnostic::{Diagnostic, Location, SimMessage};
pub use severity::Severity;
pub use parse::parse_logic_value;
pub use radix::{Radix, default_radix, format_logic, format_value, format_vhdl_time};
pub use time::SimTime;
pub use value::{Logic, Value, parse_logic_vector};

/// Name shown in window titles and `--version` output. Placeholder until
/// the product has a real name.
pub const PRODUCT_NAME: &str = "NVC Studio";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_is_set() {
        assert_eq!(PRODUCT_NAME, "NVC Studio");
    }
}
