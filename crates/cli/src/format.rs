//! Turning parsed nvc output into text for the terminal.

use nvc::OutputItem;
use studio_core::{Diagnostic, SimMessage};

/// Formats one item for printing. May be several lines long.
pub fn format_item(item: &OutputItem) -> String {
    match item {
        OutputItem::Diagnostic(diagnostic) => format_diagnostic(diagnostic),
        OutputItem::SimMessage(message) => format_sim_message(message),
        OutputItem::Text(text) => text.clone(),
    }
}

/// `file:line:col: error: message`, then each note indented below it.
/// Editors and terminals recognise the first part as a clickable location.
fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    let mut text = match &diagnostic.location {
        Some(location) => format!("{location}: {}: {}", diagnostic.severity, diagnostic.message),
        None => format!("{}: {}", diagnostic.severity, diagnostic.message),
    };
    for note in &diagnostic.notes {
        text.push_str("\n    = ");
        text.push_str(note);
    }
    text
}

/// `[15 ns] error: message  (at file:line:col)`.
fn format_sim_message(message: &SimMessage) -> String {
    let time = if message.delta == 0 {
        message.time.to_string()
    } else {
        format!("{} +{} delta", message.time, message.delta)
    };
    let mut text = format!("[{time}] {}: {}", message.severity, message.message);
    if let Some(location) = &message.location {
        text.push_str(&format!("  (at {location})"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use studio_core::{Location, Severity, SimTime};

    fn location() -> Option<Location> {
        Some(Location {
            file: PathBuf::from("tb.vhd"),
            line: 12,
            column: Some(9),
        })
    }

    #[test]
    fn diagnostic_with_location_and_notes() {
        let item = OutputItem::Diagnostic(Diagnostic {
            severity: Severity::Error,
            message: "no visible declaration for B".to_string(),
            location: location(),
            notes: vec!["did you mean A?".to_string()],
            raw: vec![],
        });
        assert_eq!(
            format_item(&item),
            "tb.vhd:12:9: error: no visible declaration for B\n    = did you mean A?"
        );
    }

    #[test]
    fn diagnostic_without_location() {
        let item = OutputItem::Diagnostic(Diagnostic {
            severity: Severity::Fatal,
            message: "cannot find unit WORK.TB".to_string(),
            location: None,
            notes: vec![],
            raw: vec![],
        });
        assert_eq!(format_item(&item), "fatal: cannot find unit WORK.TB");
    }

    #[test]
    fn sim_message_shows_time_and_location() {
        let mut message = SimMessage {
            severity: Severity::Warning,
            time: SimTime::parse("15001ps").unwrap(),
            delta: 0,
            message: "careful".to_string(),
            location: location(),
            raw: String::new(),
        };
        assert_eq!(
            format_item(&OutputItem::SimMessage(message.clone())),
            "[15001 ps] warning: careful  (at tb.vhd:12:9)"
        );

        message.delta = 2;
        message.location = None;
        assert_eq!(
            format_item(&OutputItem::SimMessage(message)),
            "[15001 ps +2 delta] warning: careful"
        );
    }

    #[test]
    fn text_is_printed_unchanged() {
        assert_eq!(format_item(&OutputItem::Text("  raw  ".to_string())), "  raw  ");
    }
}
