//! End-to-end tests that run the real nvc on designs in tests/vhdl/.
//!
//! If nvc isn't installed these tests print a message and pass, so the rest
//! of the suite still works. CI sets `NVC_REQUIRED=1`, which turns a missing
//! nvc into a failure so the tests can't be skipped by accident there.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use nvc::{
    CancelToken, Library, Nvc, NvcInstall, NvcOptions, OutputItem, RunOptions, VhdlStandard,
    locate_nvc,
};
use studio_core::{Severity, SimTime};

fn find_nvc_or_skip() -> Option<NvcInstall> {
    match locate_nvc(None) {
        Ok(install) => Some(install),
        Err(error) if std::env::var_os("NVC_REQUIRED").is_some() => {
            panic!("NVC_REQUIRED is set but nvc wasn't found: {error}")
        }
        Err(error) => {
            eprintln!("skipping: {error}");
            None
        }
    }
}

/// A fresh, empty folder for one test's work library.
fn scratch_dir(test_name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(test_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn vhdl(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/vhdl")
        .join(relative)
}

/// An `Nvc` whose work library lives in a nested folder that doesn't exist
/// yet, which also checks that we create the parent folder for nvc.
fn nvc_in(install: &NvcInstall, dir: &Path) -> Nvc {
    let work = Library {
        name: "work".to_string(),
        dir: dir.join("libs").join("work"),
    };
    let options = NvcOptions::new(VhdlStandard::Vhdl2008, work);
    Nvc::new(install, options, dir)
}

fn ignore(_: &OutputItem) {}

#[test]
fn counter_runs_end_to_end() {
    let Some(install) = find_nvc_or_skip() else { return };
    let dir = scratch_dir("counter_runs_end_to_end");
    let nvc = nvc_in(&install, &dir);
    let cancel = CancelToken::new();

    let analysed = nvc.analyse(&[vhdl("counter.vhd")], &cancel, &mut ignore).unwrap();
    assert!(analysed.succeeded(), "{analysed:?}");

    let elaborated = nvc.elaborate("counter_tb", &[], &cancel, &mut ignore).unwrap();
    assert!(elaborated.succeeded(), "{elaborated:?}");

    let run_options = RunOptions {
        stop_time: None,
        wave_file: Some(dir.join("counter_tb.fst")),
    };
    let mut live = Vec::new();
    let ran = nvc
        .run("counter_tb", &run_options, &cancel, &mut |item| live.push(item.clone()))
        .unwrap();
    assert!(ran.succeeded(), "{ran:?}");
    assert_eq!(live, ran.items, "callback should see the same items");

    let [OutputItem::SimMessage(message)] = ran.items.as_slice() else {
        panic!("expected one report, got {:?}", ran.items);
    };
    assert_eq!(message.message, "count = 10");
    assert_eq!(message.time, SimTime::parse("120ns").unwrap());
    assert!(dir.join("counter_tb.fst").is_file(), "waveform file not written");
}

#[test]
fn broken_file_gives_located_errors() {
    let Some(install) = find_nvc_or_skip() else { return };
    let dir = scratch_dir("broken_file_gives_located_errors");
    let nvc = nvc_in(&install, &dir);

    let report = nvc
        .analyse(&[vhdl("broken/multiple_errors.vhd")], &CancelToken::new(), &mut ignore)
        .unwrap();
    assert!(!report.succeeded());
    assert_eq!(report.process.exit_code, Some(1));
    assert_eq!(report.error_count(), 2);

    let lines: Vec<(u32, Option<u32>)> = report
        .items
        .iter()
        .filter_map(|item| match item {
            OutputItem::Diagnostic(d) => d.location.as_ref().map(|l| (l.line, l.column)),
            _ => None,
        })
        .collect();
    assert_eq!(lines, [(12, Some(10)), (13, Some(10))]);
}

#[test]
fn simulation_messages_and_failure_exit_code() {
    let Some(install) = find_nvc_or_skip() else { return };
    let dir = scratch_dir("simulation_messages_and_failure_exit_code");
    let nvc = nvc_in(&install, &dir);
    let cancel = CancelToken::new();

    nvc.analyse(&[vhdl("sim/messages_tb.vhd")], &cancel, &mut ignore).unwrap();
    nvc.elaborate("messages_tb", &[], &cancel, &mut ignore).unwrap();
    let report = nvc
        .run("messages_tb", &RunOptions::default(), &cancel, &mut ignore)
        .unwrap();

    assert_eq!(report.process.exit_code, Some(1), "a failure should exit 1");
    let severities: Vec<Severity> = report.items.iter().filter_map(OutputItem::severity).collect();
    assert_eq!(
        severities,
        [
            Severity::Note,
            Severity::Warning,
            Severity::Error,
            Severity::Error,
            Severity::Error,
            Severity::Failure
        ]
    );
}

#[test]
fn stop_time_ends_an_endless_simulation() {
    let Some(install) = find_nvc_or_skip() else { return };
    let dir = scratch_dir("stop_time_ends_an_endless_simulation");
    let nvc = nvc_in(&install, &dir);
    let cancel = CancelToken::new();

    nvc.analyse(&[vhdl("sim/endless_tb.vhd")], &cancel, &mut ignore).unwrap();
    nvc.elaborate("endless_tb", &[], &cancel, &mut ignore).unwrap();
    let run_options = RunOptions {
        stop_time: SimTime::parse("1us"),
        wave_file: None,
    };
    let report = nvc.run("endless_tb", &run_options, &cancel, &mut ignore).unwrap();
    assert!(report.succeeded(), "{report:?}");
}

#[test]
fn cancel_stops_a_running_simulation() {
    let Some(install) = find_nvc_or_skip() else { return };
    let dir = scratch_dir("cancel_stops_a_running_simulation");
    let nvc = nvc_in(&install, &dir);
    let cancel = CancelToken::new();

    nvc.analyse(&[vhdl("sim/endless_tb.vhd")], &cancel, &mut ignore).unwrap();
    nvc.elaborate("endless_tb", &[], &cancel, &mut ignore).unwrap();

    // Press "Stop" from another thread after half a second.
    let stop_button = cancel.clone();
    let presser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        stop_button.cancel();
    });

    let started = Instant::now();
    let report = nvc
        .run("endless_tb", &RunOptions::default(), &cancel, &mut ignore)
        .unwrap();
    presser.join().unwrap();

    assert!(report.process.cancelled);
    assert!(!report.succeeded());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?} to stop",
        started.elapsed()
    );
}
