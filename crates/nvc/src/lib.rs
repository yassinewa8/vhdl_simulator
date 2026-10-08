//! Talks to the NVC simulator by running the `nvc` program as a separate
//! process and reading what it prints.
//!
//! NVC is GPLv3: we only ever run it as a subprocess. Never link libnvc or
//! copy NVC source into this crate (see CLAUDE.md).
//!
//! Planned for Phase 1: locating nvc, running analyse/elaborate/run with
//! streamed output and cancel, and parsing diagnostics.

/// Name of the NVC executable we look for on PATH.
pub const NVC_PROGRAM: &str = "nvc";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_name_is_nvc() {
        assert_eq!(NVC_PROGRAM, "nvc");
    }
}
