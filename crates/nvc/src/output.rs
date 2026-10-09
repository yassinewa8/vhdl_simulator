//! One piece of parsed NVC output.

use studio_core::{Diagnostic, Severity, SimMessage};

/// What a line (or block of lines) of NVC output turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputItem {
    /// An error, warning or note from analysis or elaboration.
    Diagnostic(Diagnostic),
    /// A report/assert message from a running simulation.
    SimMessage(SimMessage),
    /// A line the parsers didn't recognise. Kept so nothing is ever lost.
    Text(String),
}

impl OutputItem {
    /// Severity of the item, or `None` for plain text.
    pub fn severity(&self) -> Option<Severity> {
        match self {
            OutputItem::Diagnostic(diagnostic) => Some(diagnostic.severity),
            OutputItem::SimMessage(message) => Some(message.severity),
            OutputItem::Text(_) => None,
        }
    }

    /// True if this item is an error, failure or fatal message.
    pub fn is_error(&self) -> bool {
        self.severity().is_some_and(Severity::is_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_no_severity() {
        let item = OutputItem::Text("hello".to_string());
        assert_eq!(item.severity(), None);
        assert!(!item.is_error());
    }

    #[test]
    fn diagnostic_reports_its_severity() {
        let item = OutputItem::Diagnostic(Diagnostic {
            severity: Severity::Error,
            message: "oops".to_string(),
            location: None,
            notes: vec![],
            raw: vec![],
        });
        assert_eq!(item.severity(), Some(Severity::Error));
        assert!(item.is_error());
    }
}
