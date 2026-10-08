//! Waveform loading and querying. No GUI code: the app crate draws, this
//! crate only answers questions like "what changed between t1 and t2".
//!
//! Planned for Phase 3: lazy FST/VCD loading and the query API.

/// Waveform file extensions we plan to support.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["fst", "vcd"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_fst_and_vcd() {
        assert!(SUPPORTED_EXTENSIONS.contains(&"fst"));
        assert!(SUPPORTED_EXTENSIONS.contains(&"vcd"));
    }
}
