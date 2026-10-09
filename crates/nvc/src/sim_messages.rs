//! Parser for simulation output (NVC's "compact" message style).
//!
//! We run simulations with `--messages=compact`, which prints one line per
//! message:
//!
//! ```text
//! tests/vhdl/sim/messages_tb.vhd:12:9: warning: 10ns+0: a warning message
//! tests/vhdl/sim/messages_tb.vhd:19:9: failure: 3015001ps+0: a failure message
//! fatal: cannot find unit WORK.TB in library WORK
//! ```
//!
//! The pattern is `FILE:LINE:COLUMN: severity: TIME+DELTA: message`. The
//! location is missing for some messages, and messages that aren't from a
//! report/assert (e.g. elaboration errors) have no time. Any line that
//! doesn't fit the pattern is kept as plain text.

use std::path::PathBuf;

use studio_core::{Diagnostic, Location, Severity, SimMessage, SimTime};

use crate::output::OutputItem;

/// Parses one line of compact-style output.
pub fn parse_compact_line(line: &str) -> OutputItem {
    let line = line.trim_end_matches(['\r', '\n']);
    let text = || OutputItem::Text(line.to_string());

    let Some((location_text, severity, rest)) = split_at_severity(line) else {
        return text();
    };

    let location = if location_text.is_empty() {
        None
    } else {
        match parse_location(location_text) {
            Some(location) => Some(location),
            // Something before the severity that isn't a location: we
            // can't be sure what this is, so keep it as text.
            None => return text(),
        }
    };

    match split_time(rest) {
        Some((time, delta, message)) => OutputItem::SimMessage(SimMessage {
            severity,
            time,
            delta,
            message: message.to_string(),
            location,
            raw: line.to_string(),
        }),
        None => OutputItem::Diagnostic(Diagnostic {
            severity,
            message: rest.to_string(),
            location,
            notes: vec![],
            raw: vec![line.to_string()],
        }),
    }
}

/// Parses a whole piece of compact-style output at once.
pub fn parse_compact_output(text: &str) -> Vec<OutputItem> {
    text.lines().map(parse_compact_line).collect()
}

/// Splits `FILE:LINE:COL: severity: rest` into its three parts.
///
/// Takes the *first* `: severity: ` in the line, so a message that happens
/// to contain `: error: ` itself doesn't confuse us.
fn split_at_severity(line: &str) -> Option<(&str, Severity, &str)> {
    // Message with no location, e.g. "fatal: cannot find unit ..."
    for severity in Severity::ALL {
        let prefix = format!("{}: ", severity.as_str());
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Some(("", severity, rest));
        }
    }

    let mut earliest: Option<(usize, Severity, usize)> = None;
    for severity in Severity::ALL {
        let pattern = format!(": {}: ", severity.as_str());
        if let Some(index) = line.find(&pattern) {
            let is_earlier = earliest.is_none_or(|(best, _, _)| index < best);
            if is_earlier {
                earliest = Some((index, severity, pattern.len()));
            }
        }
    }

    let (index, severity, pattern_length) = earliest?;
    Some((&line[..index], severity, &line[index + pattern_length..]))
}

/// Reads `FILE:LINE:COLUMN`, splitting from the right because Windows
/// paths contain a colon after the drive letter.
fn parse_location(text: &str) -> Option<Location> {
    let (rest, column) = text.rsplit_once(':')?;
    let (file, line) = rest.rsplit_once(':')?;
    if file.is_empty() {
        return None;
    }
    Some(Location {
        file: PathBuf::from(file),
        line: line.parse().ok()?,
        column: Some(column.parse().ok()?),
    })
}

/// Splits `15001ps+0: message` into time, delta and message.
fn split_time(text: &str) -> Option<(SimTime, u32, &str)> {
    let (time_and_delta, message) = text.split_once(": ")?;
    let (time, delta) = time_and_delta.split_once('+')?;
    Some((SimTime::parse(time)?, delta.parse().ok()?, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_report_with_location() {
        let item = parse_compact_line("tb.vhd:12:9: warning: 10ns+0: a warning message");
        let OutputItem::SimMessage(message) = item else {
            panic!("expected a sim message, got {item:?}");
        };
        assert_eq!(message.severity, Severity::Warning);
        assert_eq!(message.time, SimTime::parse("10ns").unwrap());
        assert_eq!(message.delta, 0);
        assert_eq!(message.message, "a warning message");
        assert_eq!(
            message.location,
            Some(Location {
                file: PathBuf::from("tb.vhd"),
                line: 12,
                column: Some(9),
            })
        );
    }

    #[test]
    fn keeps_the_delta_cycle() {
        let OutputItem::SimMessage(message) = parse_compact_line("a.vhd:1:1: note: 5ns+3: hi") else {
            panic!("expected a sim message");
        };
        assert_eq!(message.delta, 3);
    }

    #[test]
    fn windows_absolute_path() {
        let OutputItem::SimMessage(message) =
            parse_compact_line("C:/proj/tb.vhd:16:16: error: 15001ps+0: boom")
        else {
            panic!("expected a sim message");
        };
        assert_eq!(message.location.unwrap().file, PathBuf::from("C:/proj/tb.vhd"));
    }

    #[test]
    fn message_containing_colons() {
        let OutputItem::SimMessage(message) =
            parse_compact_line("a.vhd:1:1: error: 1ns+0: expected: 5, got: error: 7")
        else {
            panic!("expected a sim message");
        };
        assert_eq!(message.severity, Severity::Error);
        assert_eq!(message.message, "expected: 5, got: error: 7");
    }

    #[test]
    fn no_location_and_no_time_is_a_diagnostic() {
        let item = parse_compact_line("fatal: cannot find unit WORK.TB in library WORK");
        let OutputItem::Diagnostic(diagnostic) = item else {
            panic!("expected a diagnostic, got {item:?}");
        };
        assert_eq!(diagnostic.severity, Severity::Fatal);
        assert_eq!(diagnostic.location, None);
        assert_eq!(diagnostic.message, "cannot find unit WORK.TB in library WORK");
    }

    #[test]
    fn location_but_no_time_is_a_diagnostic() {
        let item = parse_compact_line("a.vhd:8:14: fatal: (init): value 0 outside of range");
        let OutputItem::Diagnostic(diagnostic) = item else {
            panic!("expected a diagnostic, got {item:?}");
        };
        assert_eq!(diagnostic.location.unwrap().line, 8);
        assert_eq!(diagnostic.message, "(init): value 0 outside of range");
    }

    #[test]
    fn stats_line_is_a_note_without_time() {
        let item = parse_compact_line("note: setup:15ms run:0ms user:0ms");
        assert!(matches!(item, OutputItem::Diagnostic(_)), "got {item:?}");
    }

    #[test]
    fn other_lines_are_kept_as_text() {
        for line in ["", "hello world", "something: else: here", "x:y: error: msg"] {
            assert_eq!(parse_compact_line(line), OutputItem::Text(line.to_string()));
        }
    }

    #[test]
    fn strips_windows_line_ending() {
        let item = parse_compact_line("a.vhd:1:1: note: 0ms+0: hi\r");
        let OutputItem::SimMessage(message) = item else {
            panic!("expected a sim message");
        };
        assert_eq!(message.message, "hi");
        assert!(!message.raw.ends_with('\r'));
    }
}
