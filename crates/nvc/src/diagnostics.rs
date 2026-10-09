//! Parser for analysis and elaboration output (NVC's "full" message style).
//!
//! A diagnostic is a block of lines like this:
//!
//! ```text
//! ** Error: no visible declaration for B
//!     > tests/vhdl/broken/undeclared.vhd:11
//!     |
//!  11 |     a <= b;
//!     |          ^ did you mean A?
//! ```
//!
//! - The header starts with `** Severity:`.
//! - Every following line of the block is indented.
//! - `> file:line` gives the location. There's no column, so we work it out
//!   from where the `^` marker sits under the source line.
//! - Text after the `^` markers ("did you mean A?") and `= Note:` lines
//!   become notes.
//!
//! Lines that aren't part of a diagnostic are kept as plain text. See
//! `docs/nvc-cli-notes.md` for more examples.

use std::path::PathBuf;

use studio_core::{Diagnostic, Location, Severity};

use crate::output::OutputItem;

/// Turns full-style NVC output into [`OutputItem`]s, one line at a time.
///
/// Because a diagnostic spans several lines, it is only returned once the
/// next one starts or [`finish`](DiagnosticParser::finish) is called.
#[derive(Debug, Default)]
pub struct DiagnosticParser {
    /// Lines of the diagnostic currently being collected, header first.
    current_block: Vec<String>,
}

impl DiagnosticParser {
    pub fn new() -> DiagnosticParser {
        DiagnosticParser::default()
    }

    /// Feeds in one line of output. Returns any items that are now complete.
    pub fn push_line(&mut self, line: &str) -> Vec<OutputItem> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut completed = Vec::new();

        if parse_header(line).is_some() {
            // A new diagnostic starts, so the previous one is finished.
            completed.extend(self.finish());
            self.current_block.push(line.to_string());
        } else if !self.current_block.is_empty() && line.starts_with(' ') {
            self.current_block.push(line.to_string());
        } else {
            completed.extend(self.finish());
            completed.push(OutputItem::Text(line.to_string()));
        }
        completed
    }

    /// Call when the output has ended. Returns the last diagnostic, if any.
    pub fn finish(&mut self) -> Option<OutputItem> {
        if self.current_block.is_empty() {
            return None;
        }
        let block = std::mem::take(&mut self.current_block);
        Some(parse_block(block))
    }
}

/// Parses a whole piece of full-style output at once.
pub fn parse_full_output(text: &str) -> Vec<OutputItem> {
    let mut parser = DiagnosticParser::new();
    let mut items = Vec::new();
    for line in text.lines() {
        items.extend(parser.push_line(line));
    }
    items.extend(parser.finish());
    items
}

/// Reads a header line such as `** Error: message`.
fn parse_header(line: &str) -> Option<(Severity, &str)> {
    let rest = line.strip_prefix("** ")?;
    let (severity, message) = rest.split_once(": ")?;
    Some((Severity::parse(severity)?, message))
}

/// Turns the collected lines of one diagnostic into a [`Diagnostic`].
fn parse_block(lines: Vec<String>) -> OutputItem {
    let Some((severity, message)) = parse_header(&lines[0]) else {
        unreachable!("a block always starts with a valid header");
    };

    let mut location: Option<Location> = None;
    let mut notes = Vec::new();
    // The line number of the source line shown most recently in the snippet,
    // so we know which line a `^` marker points at.
    let mut snippet_line: Option<u32> = None;

    for line in &lines[1..] {
        let trimmed = line.trim();

        if let Some(file_and_line) = trimmed.strip_prefix("> ") {
            // The first location is the main one; keep any others as notes.
            if location.is_none()
                && let Some(parsed) = parse_file_and_line(file_and_line)
            {
                location = Some(parsed);
            } else {
                notes.push(trimmed.to_string());
            }
        } else if let Some(note) = trimmed.strip_prefix("= ") {
            notes.push(note.to_string());
        } else if let Some(number) = snippet_line_number(trimmed) {
            snippet_line = Some(number);
        } else if trimmed.starts_with('|') {
            let Some((column, label)) = find_marker(line) else {
                continue; // an empty "|" spacer line
            };
            let primary_line = location.as_ref().map(|location| location.line);
            let on_primary_line = snippet_line.is_some() && snippet_line == primary_line;

            if on_primary_line && let Some(location) = location.as_mut() {
                location.column.get_or_insert(column);
            }
            if !label.is_empty() {
                match snippet_line {
                    Some(number) if !on_primary_line => {
                        notes.push(format!("line {number}: {label}"));
                    }
                    _ => notes.push(label.to_string()),
                }
            }
        } else if !trimmed.is_empty() {
            // Something we don't recognise: keep it rather than lose it.
            notes.push(trimmed.to_string());
        }
    }

    OutputItem::Diagnostic(Diagnostic {
        severity,
        message: message.to_string(),
        location,
        notes,
        raw: lines,
    })
}

/// Reads `path/to/file.vhd:11`. Splits at the last colon, because Windows
/// paths contain a colon after the drive letter (`C:/...`).
fn parse_file_and_line(text: &str) -> Option<Location> {
    let (file, line) = text.rsplit_once(':')?;
    Some(Location {
        file: PathBuf::from(file),
        line: line.parse().ok()?,
        column: None,
    })
}

/// For a snippet source line such as `11 |     a <= b;` returns 11.
fn snippet_line_number(trimmed: &str) -> Option<u32> {
    let (number, _source) = trimmed.split_once(" |")?;
    number.parse().ok()
}

/// For a marker line such as `    |          ^ did you mean A?` returns the
/// 1-based column the `^` points at and the label after it.
///
/// The source line above it looks like ` 11 |     a <= b;`: after the `|`
/// comes one space, then the source text. So the `^`'s position counted
/// from just after the `|` is exactly the 1-based column.
fn find_marker(line: &str) -> Option<(u32, &str)> {
    let (_gutter, after_bar) = line.split_once('|')?;
    let caret_index = after_bar.find('^')?;
    let column = after_bar[..caret_index].chars().count();
    let label = after_bar[caret_index..].trim_start_matches('^').trim();
    Some((u32::try_from(column).ok()?, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_diagnostic(text: &str) -> Diagnostic {
        let items = parse_full_output(text);
        assert_eq!(items.len(), 1, "expected one item, got {items:?}");
        match items.into_iter().next() {
            Some(OutputItem::Diagnostic(diagnostic)) => diagnostic,
            other => panic!("expected a diagnostic, got {other:?}"),
        }
    }

    #[test]
    fn parses_header_location_column_and_hint() {
        let diagnostic = only_diagnostic(
            "** Error: no visible declaration for B\n\
             \x20   > src/a.vhd:11\n\
             \x20   |\n\
             \x2011 |     a <= b;\n\
             \x20   |          ^ did you mean A?\n",
        );
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.message, "no visible declaration for B");
        assert_eq!(
            diagnostic.location,
            Some(Location {
                file: PathBuf::from("src/a.vhd"),
                line: 11,
                column: Some(10),
            })
        );
        assert_eq!(diagnostic.notes, vec!["did you mean A?"]);
        assert_eq!(diagnostic.raw.len(), 5);
    }

    #[test]
    fn header_without_location() {
        let diagnostic = only_diagnostic("** Fatal: cannot find unit WORK.TB in library WORK\n");
        assert_eq!(diagnostic.severity, Severity::Fatal);
        assert_eq!(diagnostic.location, None);
        assert!(diagnostic.notes.is_empty());
    }

    #[test]
    fn keeps_equals_notes() {
        let diagnostic = only_diagnostic(
            "** Error: shared variable SV must have protected type\n\
             \x20  > w.vhd:9\n\
             \x20  |\n\
             \x209 |     shared variable sv : integer;\n\
             \x20  |                     ^^\n\
             \x20  |\n\
             \x20  = Note: the --relaxed option downgrades this to a warning\n",
        );
        assert_eq!(diagnostic.location.unwrap().column, Some(21));
        assert_eq!(
            diagnostic.notes,
            vec!["Note: the --relaxed option downgrades this to a warning"]
        );
    }

    #[test]
    fn windows_drive_letter_in_path() {
        let diagnostic = only_diagnostic("** Error: bad\n    > C:/work/a.vhd:3\n");
        let location = diagnostic.location.unwrap();
        assert_eq!(location.file, PathBuf::from("C:/work/a.vhd"));
        assert_eq!(location.line, 3);
    }

    #[test]
    fn handles_windows_line_endings() {
        let diagnostic = only_diagnostic("** Warning: careful\r\n    > a.vhd:2\r\n");
        assert_eq!(diagnostic.message, "careful");
        assert_eq!(diagnostic.location.unwrap().line, 2);
    }

    #[test]
    fn unrecognised_lines_become_text() {
        let items = parse_full_output("hello\n** Note: hi\nbye\n");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], OutputItem::Text("hello".to_string()));
        assert!(matches!(items[1], OutputItem::Diagnostic(_)));
        assert_eq!(items[2], OutputItem::Text("bye".to_string()));
    }

    #[test]
    fn unknown_severity_is_text_not_a_diagnostic() {
        let items = parse_full_output("** Info: something\n");
        assert_eq!(items, vec![OutputItem::Text("** Info: something".to_string())]);
    }

    #[test]
    fn streaming_returns_a_diagnostic_when_the_next_one_starts() {
        let mut parser = DiagnosticParser::new();
        assert!(parser.push_line("** Error: first").is_empty());
        assert!(parser.push_line("    > a.vhd:1").is_empty());
        let completed = parser.push_line("** Error: second");
        assert_eq!(completed.len(), 1);
        assert!(parser.finish().is_some());
        assert!(parser.finish().is_none());
    }

    #[test]
    fn marker_column_counts_from_after_the_bar() {
        assert_eq!(find_marker("    |          ^ did you mean A?"), Some((10, "did you mean A?")));
        assert_eq!(find_marker("   |     ^^^^"), Some((5, "")));
        assert_eq!(find_marker("    |"), None);
    }
}
