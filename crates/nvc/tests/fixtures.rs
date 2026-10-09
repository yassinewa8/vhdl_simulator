//! Parser tests against real NVC output captured by
//! `scripts/capture-nvc-fixtures.sh`. These don't need nvc installed.

use std::path::PathBuf;

use nvc::{OutputItem, parse_compact_output, parse_full_output};
use studio_core::{Diagnostic, Location, Severity, SimMessage, SimTime};

/// Reads `tests/fixtures/nvc/<name>` from the repository root.
fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/nvc")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn diagnostics(items: Vec<OutputItem>) -> Vec<Diagnostic> {
    items
        .into_iter()
        .map(|item| match item {
            OutputItem::Diagnostic(diagnostic) => diagnostic,
            other => panic!("expected only diagnostics, got {other:?}"),
        })
        .collect()
}

fn sim_messages(items: Vec<OutputItem>) -> Vec<SimMessage> {
    items
        .into_iter()
        .map(|item| match item {
            OutputItem::SimMessage(message) => message,
            other => panic!("expected only sim messages, got {other:?}"),
        })
        .collect()
}

fn location(file: &str, line: u32, column: u32) -> Option<Location> {
    Some(Location {
        file: PathBuf::from(file),
        line,
        column: Some(column),
    })
}

/// The column worked out from the `^` marker in full style must match the
/// column NVC prints in compact style, for every analysis fixture.
#[test]
fn full_style_columns_match_compact_style() {
    for name in [
        "analyse_syntax_error",
        "analyse_undeclared",
        "analyse_type_mismatch",
        "analyse_multiple_errors",
        "analyse_missing_unit",
        "analyse_warnings",
        "elab_bad_generic",
    ] {
        let full = diagnostics(parse_full_output(&fixture(&format!("{name}.full.txt"))));
        let compact = diagnostics(parse_compact_output(&fixture(&format!("{name}.compact.txt"))));
        assert_eq!(full.len(), compact.len(), "{name}: different number of messages");
        for (full, compact) in full.iter().zip(&compact) {
            assert_eq!(full.severity, compact.severity, "{name}");
            assert_eq!(full.message, compact.message, "{name}");
            assert_eq!(full.location, compact.location, "{name}: {}", full.message);
        }
    }
}

#[test]
fn undeclared_signal_keeps_the_hint() {
    let found = diagnostics(parse_full_output(&fixture("analyse_undeclared.full.txt")));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].message, "no visible declaration for B");
    assert_eq!(
        found[0].location,
        location("tests/vhdl/broken/undeclared.vhd", 11, 10)
    );
    assert_eq!(found[0].notes, ["did you mean A?"]);
}

#[test]
fn syntax_error() {
    let found = diagnostics(parse_full_output(&fixture("analyse_syntax_error.full.txt")));
    assert_eq!(found.len(), 1);
    assert!(found[0].message.starts_with("unexpected begin"));
    assert_eq!(found[0].notes, ["this token was unexpected"]);
}

#[test]
fn multiple_errors_are_all_reported() {
    let found = diagnostics(parse_full_output(&fixture("analyse_multiple_errors.full.txt")));
    let messages: Vec<&str> = found.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "no visible declaration for MISSING_ONE",
            "no visible declaration for MISSING_TWO"
        ]
    );
}

#[test]
fn warnings_with_multi_line_snippet() {
    let found = diagnostics(parse_full_output(&fixture("analyse_warnings.full.txt")));
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|d| d.severity == Severity::Warning));

    let sensitivity = &found[1];
    assert_eq!(
        sensitivity.location,
        location("tests/vhdl/broken/warnings.vhd", 15, 20)
    );
    assert_eq!(
        sensitivity.notes,
        ["line 13: missing from sensitivity list", "read here"]
    );
}

#[test]
fn elaboration_fatal_without_location() {
    let found = diagnostics(parse_full_output(&fixture("elab_missing_top.full.txt")));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Fatal);
    assert_eq!(found[0].location, None);
    assert_eq!(found[0].message, "cannot find unit WORK.NO_SUCH_TB in library WORK");
}

#[test]
fn elaboration_warning_for_unused_generic() {
    let found = diagnostics(parse_full_output(&fixture("elab_unused_generic.full.txt")));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].message, "generic value for NOPE not used");
}

#[test]
fn simulation_messages_have_severity_time_and_statement_location() {
    let found = sim_messages(parse_compact_output(&fixture("run_messages.compact.txt")));
    let file = "tests/vhdl/sim/messages_tb.vhd";
    let summary: Vec<(Severity, SimTime, &str, Option<Location>)> = found
        .iter()
        .map(|m| (m.severity, m.time, m.message.as_str(), m.location.clone()))
        .collect();
    let at = |text: &str| SimTime::parse(text).unwrap();

    assert_eq!(
        summary,
        [
            (Severity::Note, at("0ns"), "a note message", location(file, 10, 9)),
            (Severity::Warning, at("10ns"), "a warning message", location(file, 12, 9)),
            (Severity::Error, at("15ns"), "an error message", location(file, 14, 9)),
            (Severity::Error, at("15001ps"), "assertion with message", location(file, 16, 16)),
            (Severity::Error, at("15001ps"), "Assertion violation.", location(file, 17, 16)),
            (Severity::Failure, at("3015001ps"), "a failure message", location(file, 19, 9)),
        ]
    );
}

#[test]
fn counter_testbench_report() {
    let found = sim_messages(parse_compact_output(&fixture("run_counter.compact.txt")));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Note);
    assert_eq!(found[0].time, SimTime::parse("120ns").unwrap());
    assert_eq!(found[0].message, "count = 10");
}

#[test]
fn every_fixture_line_is_kept() {
    // Nothing may be dropped: each raw line ends up in exactly one item.
    for name in ["analyse_warnings.full.txt", "elab_bad_generic.full.txt"] {
        let text = fixture(name);
        let items = parse_full_output(&text);
        let kept: usize = items
            .iter()
            .map(|item| match item {
                OutputItem::Diagnostic(d) => d.raw.len(),
                _ => 1,
            })
            .sum();
        assert_eq!(kept, text.lines().count(), "{name}");
    }
}
