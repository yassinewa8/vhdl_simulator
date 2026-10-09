//! How serious a message is, from a note up to a fatal error.

use std::fmt;

/// Severity of a diagnostic or simulation message.
///
/// The first four match VHDL's `severity_level` (note, warning, error,
/// failure). `Fatal` is NVC's own level for problems that stop it
/// immediately, such as a missing design unit.
///
/// The order matters: `Note < Warning < Error < Failure < Fatal`, so
/// severities can be compared with `<` and `>=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Note,
    Warning,
    Error,
    Failure,
    Fatal,
}

impl Severity {
    /// All severities, lowest first.
    pub const ALL: [Severity; 5] = [
        Severity::Note,
        Severity::Warning,
        Severity::Error,
        Severity::Failure,
        Severity::Fatal,
    ];

    /// Reads a severity name, ignoring case: `"Error"`, `"error"`, `"ERROR"`.
    pub fn parse(text: &str) -> Option<Severity> {
        Severity::ALL
            .into_iter()
            .find(|severity| severity.as_str().eq_ignore_ascii_case(text))
    }

    /// Lower-case name, as NVC prints it in compact mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Failure => "failure",
            Severity::Fatal => "fatal",
        }
    }

    /// True for error, failure and fatal: anything that makes a build or
    /// simulation count as failed.
    pub fn is_error(self) -> bool {
        self >= Severity::Error
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ignores_case() {
        assert_eq!(Severity::parse("Error"), Some(Severity::Error));
        assert_eq!(Severity::parse("warning"), Some(Severity::Warning));
        assert_eq!(Severity::parse("FATAL"), Some(Severity::Fatal));
    }

    #[test]
    fn parse_rejects_unknown_names() {
        assert_eq!(Severity::parse("info"), None);
        assert_eq!(Severity::parse(""), None);
    }

    #[test]
    fn severities_are_ordered() {
        assert!(Severity::Note < Severity::Warning);
        assert!(Severity::Error < Severity::Failure);
        assert!(Severity::Failure < Severity::Fatal);
    }

    #[test]
    fn only_error_and_above_count_as_errors() {
        assert!(!Severity::Note.is_error());
        assert!(!Severity::Warning.is_error());
        assert!(Severity::Error.is_error());
        assert!(Severity::Failure.is_error());
        assert!(Severity::Fatal.is_error());
    }

    #[test]
    fn display_is_lower_case() {
        assert_eq!(Severity::Failure.to_string(), "failure");
    }
}
