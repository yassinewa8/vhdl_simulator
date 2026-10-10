//! Tests for the viewer's queries: changes in a window, the per-pixel
//! summary, and next/previous change or value.
//!
//! The first group uses the captured fixture types_tb.fst, where `sl`
//! changes every 10 ns (U, X, Z, W, -, L, H at 0, 10, ..., 60 ns).
//! The second group has nvc generate a waveform with 200,001 clock edges
//! (tests/vhdl/waves/clock_tb.vhd); it's skipped without nvc unless
//! NVC_REQUIRED is set, as in CI.

use std::path::{Path, PathBuf};
use std::process::Command;

use studio_core::{CancelToken, SimTime, Value, parse_logic_vector};
use wave::{Bucket, Change, SignalId, Waveform};

fn ns(value: u64) -> SimTime {
    SimTime::from_femtoseconds(value * 1_000_000)
}

fn logic(text: &str) -> Value {
    Value::Logic(parse_logic_vector(text).unwrap())
}

fn change(time_ns: u64, value: &str) -> Change {
    Change {
        time: ns(time_ns),
        value: logic(value),
    }
}

/// types_tb.fst with `sl` and `state` loaded.
fn types_fixture() -> (Waveform, SignalId, SignalId) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/waves/types_tb.fst");
    let mut wave = Waveform::open(&path).unwrap();
    let sl = wave.find_signal("types_tb.sl").unwrap();
    let state = wave.find_signal("types_tb.state").unwrap();
    wave.load(&[sl, state], &CancelToken::new());
    (wave, sl, state)
}

#[test]
fn changes_in_a_window_with_the_value_before_it() {
    let (wave, sl, _) = types_fixture();

    let found = wave.changes(sl, ns(15)..ns(45)).unwrap();
    assert_eq!(found.before, Some(change(10, "X")));
    assert_eq!(found.changes, [change(20, "Z"), change(30, "W"), change(40, "-")]);

    // A window starting exactly on a change includes it; one ending
    // exactly on a change doesn't.
    let found = wave.changes(sl, ns(20)..ns(30)).unwrap();
    assert_eq!(found.before, Some(change(10, "X")));
    assert_eq!(found.changes, [change(20, "Z")]);

    // Before anything happened: no previous value.
    let found = wave.changes(sl, ns(0)..ns(5)).unwrap();
    assert_eq!(found.before, None);
    assert_eq!(found.changes, [change(0, "U")]);

    // After the last change: just the value carrying on.
    let found = wave.changes(sl, ns(100)..ns(200)).unwrap();
    assert_eq!(found.before, Some(change(60, "H")));
    assert!(found.changes.is_empty());
}

#[test]
fn summary_per_pixel() {
    let (wave, sl, _) = types_fixture();

    // Six 10 ns columns: one change each.
    let summary = wave.summary(sl, ns(0)..ns(60), 6).unwrap();
    assert_eq!(summary.start, None);
    let expected = [(0, "U"), (10, "X"), (20, "Z"), (30, "W"), (40, "-"), (50, "L")];
    assert_eq!(summary.buckets, expected.map(|(t, v)| Bucket::One(change(t, v))));

    // Two 30 ns columns: three changes each, "many changes here".
    let summary = wave.summary(sl, ns(0)..ns(60), 2).unwrap();
    assert_eq!(
        summary.buckets,
        [
            Bucket::Many {
                count: 3,
                last_value: logic("Z")
            },
            Bucket::Many {
                count: 3,
                last_value: logic("L")
            },
        ]
    );

    // After the last change: quiet columns, starting from H.
    let summary = wave.summary(sl, ns(61)..ns(70), 3).unwrap();
    assert_eq!(summary.start, Some(logic("H")));
    assert_eq!(summary.buckets, [Bucket::Quiet, Bucket::Quiet, Bucket::Quiet]);
}

#[test]
fn next_and_previous_change() {
    let (wave, sl, _) = types_fixture();
    assert_eq!(wave.next_change(sl, ns(9)), Some(ns(10)));
    assert_eq!(wave.next_change(sl, ns(10)), Some(ns(20)), "strictly after");
    assert_eq!(wave.next_change(sl, ns(60)), None, "nothing after the last change");
    assert_eq!(wave.prev_change(sl, ns(11)), Some(ns(10)));
    assert_eq!(wave.prev_change(sl, ns(10)), Some(ns(0)), "strictly before");
    assert_eq!(wave.prev_change(sl, ns(0)), None);
}

#[test]
fn next_and_previous_value() {
    let (wave, sl, state) = types_fixture();
    assert_eq!(wave.next_equal(sl, ns(0), &logic("W")), Some(ns(30)));
    assert_eq!(wave.next_equal(sl, ns(30), &logic("W")), None, "only becomes W once");
    assert_eq!(wave.prev_equal(sl, ns(60), &logic("X")), Some(ns(10)));
    // Exact match: H is not 1.
    assert_eq!(wave.next_equal(sl, ns(0), &logic("1")), None);
    // Enumerations by name.
    let data = Value::Text("data".to_string());
    assert_eq!(wave.next_equal(state, ns(0), &data), Some(ns(30)));
}

#[test]
fn queries_on_an_unloaded_signal_return_nothing() {
    let (wave, _, _) = types_fixture();
    let bus = wave.find_signal("types_tb.bus8").unwrap();
    assert_eq!(wave.changes(bus, ns(0)..ns(10)), None);
    assert_eq!(wave.summary(bus, ns(0)..ns(10), 4), None);
    assert_eq!(wave.next_change(bus, ns(0)), None);
}

/// Runs nvc on clock_tb for 1 ms and opens the result, or `None` if nvc
/// isn't installed (and NVC_REQUIRED isn't set).
fn generated_clock_waveform() -> Option<(Waveform, SignalId, SignalId)> {
    let install = match nvc::locate_nvc(None) {
        Ok(install) => install,
        Err(error) if std::env::var_os("NVC_REQUIRED").is_some() => panic!("NVC_REQUIRED but: {error}"),
        Err(error) => {
            eprintln!("skipping: {error}");
            return None;
        }
    };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wave_queries_clock");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vhdl/waves/clock_tb.vhd");
    let wave_file = dir.join("clock_tb.fst");
    let status = Command::new(&install.program)
        .current_dir(&dir)
        .args(["--std=2008", "--work=work:work", "-a"])
        .arg(&source)
        .args(["-e", "clock_tb", "-r", "--stop-time=1ms", "--wave=clock_tb.fst", "clock_tb"])
        .status()
        .unwrap();
    assert!(status.success(), "nvc failed");

    let mut wave = Waveform::open(&wave_file).unwrap();
    let clk = wave.find_signal("clock_tb.clk").unwrap();
    let count = wave.find_signal("clock_tb.count").unwrap();
    wave.load(&[clk, count], &CancelToken::new());
    Some((wave, clk, count))
}

#[test]
fn busy_signal_summaries_and_searches() {
    let Some((wave, clk, count)) = generated_clock_waveform() else {
        return;
    };
    let end = ns(1_000_000);

    // Fully zoomed out: 1000 columns of 1 us, each with 200 clock edges,
    // and the counts add up to the true number of changes.
    let summary = wave.summary(clk, SimTime::ZERO..end, 1000).unwrap();
    let mut total = 0;
    for bucket in &summary.buckets {
        let Bucket::Many { count, .. } = bucket else {
            panic!("expected many changes, got {bucket:?}");
        };
        assert_eq!(*count, 200);
        total += count;
    }
    let all = wave.changes(clk, SimTime::ZERO..end).unwrap();
    assert_eq!(total, all.changes.len());
    assert_eq!(total, 200_000);

    // Zoomed in: 5 ns columns hold one edge each, alternating 0 and 1.
    let summary = wave.summary(clk, ns(1_000)..ns(1_050), 10).unwrap();
    for (column, bucket) in summary.buckets.iter().enumerate() {
        let expected = if column % 2 == 0 { "0" } else { "1" };
        assert_eq!(*bucket, Bucket::One(change(1_000 + 5 * column as u64, expected)));
    }

    assert_eq!(wave.next_change(clk, ns(1_002)), Some(ns(1_005)));
    assert_eq!(wave.prev_change(clk, end), Some(ns(999_995)));

    // The counter becomes N at (10 * N - 5) ns, wrapping every 256.
    let sixteen = logic("00010000");
    assert_eq!(wave.next_equal(count, SimTime::ZERO, &sixteen), Some(ns(155)));
    // Last time before 1 ms: count number 99,856 (99,856 mod 256 = 16).
    assert_eq!(wave.prev_equal(count, end, &sixteen), Some(ns(10 * 99_856 - 5)));
}
