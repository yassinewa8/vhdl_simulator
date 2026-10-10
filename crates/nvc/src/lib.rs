//! Talks to the NVC simulator by running the `nvc` program as a separate
//! process and reading what it prints.
//!
//! NVC is GPLv3: we only ever run it as a subprocess. Never link libnvc or
//! copy NVC source into this crate (see CLAUDE.md).
//!
//! Typical use:
//!
//! ```no_run
//! use nvc::{CancelToken, Library, Nvc, NvcOptions, RunOptions, VhdlStandard, locate_nvc};
//!
//! let install = locate_nvc(None)?;
//! let work = Library { name: "work".into(), dir: "work".into() };
//! let options = NvcOptions::new(VhdlStandard::Vhdl2008, work);
//! let nvc = Nvc::new(&install, options, ".");
//! let cancel = CancelToken::new();
//! let mut print = |item: &nvc::OutputItem| println!("{item:?}");
//!
//! nvc.analyse(&["counter.vhd".into()], &cancel, &mut print)?;
//! nvc.elaborate("counter_tb", &[], &cancel, &mut print)?;
//! nvc.run("counter_tb", &RunOptions::default(), &cancel, &mut print)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Modules, lowest level first:
//! - `locate`: find nvc and check its version
//! - `options`: build the command-line arguments for each step
//! - `runner`: run a process, stream its output line by line, cancel it
//! - `diagnostics`: parse analyse/elaborate output
//! - `sim_messages`: parse simulation output
//! - `steps`: all of the above together, one method per step
//! - `format`: parsed output as text, for printing or a log window
//! - `install`: installing OSVVM/UVVM etc. with nvc's install scripts

mod diagnostics;
mod format;
mod install;
mod locate;
mod options;
mod output;
mod runner;
mod sim_messages;
mod steps;

pub use diagnostics::{DiagnosticParser, parse_full_output};
pub use format::format_item;
pub use install::{
    Bash, InstallError, default_library_dir, default_tcllib_dir, find_bash, install_package,
    installable_packages, installed_libraries, libexec_dir,
};
pub use locate::{LocateError, MINIMUM_VERSION, NvcInstall, Version, check_version, locate_nvc};
pub use options::{Library, NvcOptions, RunOptions, VhdlStandard};
pub use output::OutputItem;
pub use runner::{CancelToken, ProcessResult, Stream, run_process, run_process_with_env};
pub use sim_messages::{parse_compact_line, parse_compact_output};
pub use steps::{Nvc, Step, StepReport};
