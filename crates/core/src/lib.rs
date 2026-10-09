//! Shared types used by every other crate in the workspace.
//!
//! - [`Severity`]: note, warning, error, failure, fatal
//! - [`SimTime`]: simulation time in femtoseconds
//! - [`Diagnostic`], [`SimMessage`], [`Location`]: structured messages from
//!   the compiler and the simulator
//!
//! Signal values (std_logic's nine values) arrive in Phase 3.
//! Nothing here may depend on the GUI.

mod diagnostic;
mod severity;
mod time;

pub use diagnostic::{Diagnostic, Location, SimMessage};
pub use severity::Severity;
pub use time::SimTime;

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
