//! Shared types used by every other crate in the workspace.
//!
//! Planned contents:
//! - diagnostics: errors/warnings with file, line, column and message (Phase 1)
//! - time values: simulation time with units, fs to sec (Phase 1/3)
//! - signal values: std_logic's nine values U X 0 1 Z W L H - (Phase 3)
//!
//! Nothing here may depend on the GUI.

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
