//! Messages from the compiler and the simulator, in structured form.

use std::fmt;
use std::path::PathBuf;

use crate::severity::Severity;
use crate::time::SimTime;

/// A position in a source file. Lines and columns count from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub file: PathBuf,
    pub line: u32,
    /// Not every message says which column; `None` when it's unknown.
    pub column: Option<u32>,
}

impl fmt::Display for Location {
    /// Writes `file:line:column`, or `file:line` without a column. This is
    /// the format most editors understand for "jump to this place".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file.display(), self.line)?;
        if let Some(column) = self.column {
            write!(f, ":{column}")?;
        }
        Ok(())
    }
}

/// An error, warning or note from analysis or elaboration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// Where the problem is. `None` for messages about the design as a
    /// whole, e.g. "cannot find unit WORK.TB".
    pub location: Option<Location>,
    /// Extra hints NVC gives, e.g. "did you mean A?".
    pub notes: Vec<String>,
    /// The original lines exactly as NVC printed them.
    pub raw: Vec<String>,
}

/// A report or assert message printed while the simulation runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimMessage {
    pub severity: Severity,
    /// Simulation time when the message was printed.
    pub time: SimTime,
    /// Delta cycle within that time step.
    pub delta: u32,
    pub message: String,
    /// The `report` or `assert` statement that printed the message.
    pub location: Option<Location>,
    /// The original line exactly as NVC printed it.
    pub raw: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_displays_like_an_editor_reference() {
        let with_column = Location {
            file: PathBuf::from("src/top.vhd"),
            line: 12,
            column: Some(5),
        };
        assert_eq!(with_column.to_string(), "src/top.vhd:12:5");

        let without_column = Location {
            column: None,
            ..with_column
        };
        assert_eq!(without_column.to_string(), "src/top.vhd:12");
    }
}
