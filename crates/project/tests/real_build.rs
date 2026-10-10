//! End-to-end tests that build and run tests/vhdl/projects/three_libs with
//! the real nvc.
//!
//! Like the nvc crate's end-to-end tests, these skip when nvc isn't
//! installed, unless `NVC_REQUIRED` is set (as it is in CI).

use std::path::{Path, PathBuf};

use nvc::{CancelToken, NvcInstall, OutputItem, locate_nvc};
use project::{BuildEvent, BuildPlan, BuildSummary, Project, plan_build, run_build, run_testbench};

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

/// A fresh copy of a test project from tests/vhdl/projects, so builds
/// don't write into the repo.
fn copy_of_project(name: &str, test_name: &str) -> Project {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/vhdl/projects").join(name);
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join(test_name);
    let _ = std::fs::remove_dir_all(&target);
    copy_folder(&source, &target);
    Project::load(&target).unwrap()
}

fn copy_of_three_libs(test_name: &str) -> Project {
    copy_of_project("three_libs", test_name)
}

fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn version(install: &NvcInstall) -> String {
    install.version.to_string()
}

/// Plans and runs a build, returning the plan, summary and every event.
fn build(project: &Project, install: &NvcInstall) -> (BuildPlan, BuildSummary, Vec<BuildEvent>) {
    let plan = plan_build(project, &version(install), false).unwrap();
    let mut events = Vec::new();
    let summary = run_build(project, &plan, install, &CancelToken::new(), &mut |event| events.push(event)).unwrap();
    (plan, summary, events)
}

/// Paths of the files that were actually compiled, in order.
fn compiled_files(events: &[BuildEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            BuildEvent::FileStarted { path, .. } => Some(path.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn edit(project: &Project, file: &str, find: &str, replace: &str) {
    let path: PathBuf = project.root.join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(find), "{find:?} not in {file}");
    std::fs::write(&path, text.replace(find, replace)).unwrap();
}

#[test]
fn builds_in_dependency_order_and_runs_the_testbench() {
    let Some(install) = find_nvc_or_skip() else { return };
    let project = copy_of_three_libs("builds_in_dependency_order");

    let (plan, summary, events) = build(&project, &install);
    assert!(summary.succeeded(), "{summary:?}\n{events:#?}");
    assert_eq!(summary.compiled, 6);
    assert_eq!(
        compiled_files(&events),
        [
            "src/b_adder.vhd",
            "util/z_math_pkg.vhd",
            "src/design_ctx.vhd",
            "src/a_top.vhd",
            "tb/top_tb.vhd",
            "util/a_math_body.vhd",
        ]
    );
    assert!(project.state_file().is_file());

    let testbench = &project.config.testbenches[0];
    let mut messages = Vec::new();
    let outcome = run_testbench(&project, &plan, testbench, &install, &CancelToken::new(), &mut |event| {
        if let BuildEvent::Output(OutputItem::SimMessage(message)) = event {
            messages.push(message.message);
        }
    })
    .unwrap();
    assert!(outcome.passed(), "{outcome:?}");
    // OFFSET = 5 from project.toml: 2 * 10 + 5.
    assert_eq!(messages, ["y = 25"]);

    // Runs write an FST waveform by default.
    let wave_file = outcome.wave_file.expect("a waveform");
    assert_eq!(wave_file, project.root.join("build").join("waves").join("top_tb.fst"));
    assert!(std::fs::metadata(&wave_file).unwrap().len() > 0);

    // ...unless switched off.
    let mut quiet = testbench.clone();
    quiet.name = Some("quiet".to_string());
    quiet.waves = false;
    let outcome = run_testbench(&project, &plan, &quiet, &install, &CancelToken::new(), &mut |_| {}).unwrap();
    assert!(outcome.passed());
    assert_eq!(outcome.wave_file, None);
    assert!(!project.root.join("build/waves/quiet.fst").exists());
}

#[test]
fn rebuild_compiles_only_what_changed_and_its_dependents() {
    let Some(install) = find_nvc_or_skip() else { return };
    let project = copy_of_three_libs("rebuild_compiles_only_what_changed");
    assert!(build(&project, &install).1.succeeded());

    // Nothing changed: nothing compiled.
    let (_, summary, events) = build(&project, &install);
    assert!(compiled_files(&events).is_empty(), "{events:#?}");
    assert_eq!(summary.up_to_date, 6);

    // Change the adder: it, the top level using it, and the testbench
    // using that are recompiled. The util library is left alone.
    edit(&project, "src/b_adder.vhd", "sum <= a + b;", "sum <= b + a;");
    let (_, summary, events) = build(&project, &install);
    assert!(summary.succeeded(), "{events:#?}");
    assert_eq!(
        compiled_files(&events),
        ["src/b_adder.vhd", "src/a_top.vhd", "tb/top_tb.vhd"]
    );

    // Deleting the output folder rebuilds everything.
    std::fs::remove_dir_all(project.root.join("build")).unwrap();
    assert_eq!(compiled_files(&build(&project, &install).2).len(), 6);
}

#[test]
fn error_skips_dependents_but_builds_independent_files() {
    let Some(install) = find_nvc_or_skip() else { return };
    let project = copy_of_three_libs("error_skips_dependents");
    edit(&project, "src/design_ctx.vhd", "use util_lib.math_pkg.all;", "use util_lib.no_such_pkg.all;");

    let (_, summary, events) = build(&project, &install);
    assert!(!summary.succeeded());
    assert_eq!(summary.failed, [PathBuf::from("src/design_ctx.vhd")]);
    assert_eq!(summary.skipped, 2, "a_top and top_tb need the context");
    assert_eq!(summary.compiled, 3, "b_adder, math_pkg and its body");

    let has_error_for_the_context = events.iter().any(|event| {
        matches!(event, BuildEvent::Output(OutputItem::Diagnostic(d))
            if d.location.as_ref().is_some_and(|l| l.file.ends_with("design_ctx.vhd")))
    });
    assert!(has_error_for_the_context, "{events:#?}");

    // After fixing it, only the failed and skipped files are compiled.
    edit(&project, "src/design_ctx.vhd", "no_such_pkg", "math_pkg");
    let (_, summary, events) = build(&project, &install);
    assert!(summary.succeeded(), "{events:#?}");
    assert_eq!(
        compiled_files(&events),
        ["src/design_ctx.vhd", "src/a_top.vhd", "tb/top_tb.vhd"]
    );
}

/// Needs OSVVM installed in ~/.nvc/lib (`cli install osvvm`). Skipped if
/// it isn't, unless `OSVVM_REQUIRED` is set.
#[test]
fn project_using_installed_osvvm_builds_and_passes() {
    let Some(install) = find_nvc_or_skip() else { return };
    let osvvm_installed = nvc::default_library_dir().is_some_and(|dir| dir.join("OSVVM.08").is_dir());
    if !osvvm_installed {
        assert!(
            std::env::var_os("OSVVM_REQUIRED").is_none(),
            "OSVVM_REQUIRED is set but OSVVM isn't installed"
        );
        eprintln!("skipping: OSVVM is not installed (run `cli install osvvm`)");
        return;
    }
    let project = copy_of_project("osvvm_tb", "project_using_installed_osvvm");

    let (plan, summary, events) = build(&project, &install);
    assert!(summary.succeeded(), "{events:#?}");

    let mut text = Vec::new();
    let outcome = run_testbench(
        &project,
        &plan,
        &project.config.testbenches[0],
        &install,
        &CancelToken::new(),
        &mut |event| {
            if let BuildEvent::Output(OutputItem::Text(line)) = event {
                text.push(line);
            }
        },
    )
    .unwrap();
    assert!(outcome.passed(), "{outcome:?}");
    // OSVVM's ReportAlerts summary, printed to stdout.
    assert!(
        text.iter().any(|line| line.contains("DONE   PASSED") && line.contains("Affirmations Checked: 100")),
        "{text:#?}"
    );
}
