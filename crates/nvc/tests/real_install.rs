//! Runs a real library install. Downloads OSVVM (into ~/.cache/nvc) and
//! compiles it into ~/.nvc/lib, so it's opt-in:
//!
//! ```text
//! cargo test -p nvc --test real_install -- --ignored
//! ```
//!
//! Needs bash, git and tclsh with Tcllib (see `nvc::install_package`).

use nvc::{
    CancelToken, InstallOptions, default_library_dir, default_tcllib_dir, find_bash, install_package, installed_libraries,
    locate_nvc,
};

#[test]
#[ignore = "downloads and compiles OSVVM; run with --ignored"]
fn installs_osvvm() {
    let install = locate_nvc(None).expect("nvc is needed for this test");
    let bash = find_bash().expect("bash is needed for this test");

    let mut lines = Vec::new();
    let result = install_package(
        &install,
        &bash,
        "osvvm",
        &InstallOptions {
            tcllib_dir: default_tcllib_dir(),
            ..InstallOptions::default()
        },
        &CancelToken::new(),
        &mut |line| lines.push(line.to_string()),
    )
    .unwrap();
    assert!(result.succeeded(), "{result:?}\n{}", lines.join("\n"));
    assert!(lines.iter().any(|line| line.contains("PASSED")), "{}", lines.join("\n"));

    let installed = installed_libraries(&default_library_dir().unwrap());
    assert!(installed.contains(&"OSVVM.08".to_string()), "{installed:?}");
}
