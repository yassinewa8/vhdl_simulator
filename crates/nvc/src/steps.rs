//! The three nvc steps (analyse, elaborate, run) with parsed output.
//!
//! This is the part other crates use. It combines the argument builder,
//! the process runner and the right parser for each step.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::diagnostics::DiagnosticParser;
use crate::locate::NvcInstall;
use crate::options::{NvcOptions, RunOptions};
use crate::output::OutputItem;
use crate::runner::{CancelToken, ProcessResult, Stream, run_process};
use crate::sim_messages::parse_compact_line;

/// One of nvc's steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Analyse,
    Elaborate,
    Run,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::Analyse => "analyse",
            Step::Elaborate => "elaborate",
            Step::Run => "run",
        })
    }
}

/// What happened in one step.
#[derive(Debug, Clone)]
pub struct StepReport {
    pub step: Step,
    pub process: ProcessResult,
    /// Everything nvc printed, parsed, in the order it arrived.
    pub items: Vec<OutputItem>,
}

impl StepReport {
    /// True if nvc exited successfully and wasn't cancelled.
    pub fn succeeded(&self) -> bool {
        self.process.succeeded()
    }

    /// Number of error, failure and fatal messages.
    pub fn error_count(&self) -> usize {
        self.items.iter().filter(|item| item.is_error()).count()
    }
}

/// Runs nvc steps for one work library.
#[derive(Debug, Clone)]
pub struct Nvc {
    pub program: PathBuf,
    pub options: NvcOptions,
    /// Folder nvc runs in. Relative paths (source files, the work library,
    /// waveform files) are relative to this.
    pub working_dir: PathBuf,
}

impl Nvc {
    pub fn new(install: &NvcInstall, options: NvcOptions, working_dir: impl Into<PathBuf>) -> Nvc {
        Nvc {
            program: install.program.clone(),
            options,
            working_dir: working_dir.into(),
        }
    }

    /// Analyses `files` into the work library.
    ///
    /// `on_item` is called with each piece of output as soon as it's parsed,
    /// e.g. to show it live in the GUI. Everything is also in the report.
    pub fn analyse(
        &self,
        files: &[PathBuf],
        cancel: &CancelToken,
        on_item: &mut dyn FnMut(&OutputItem),
    ) -> std::io::Result<StepReport> {
        self.create_work_library_parent()?;
        let args = self.options.analyse_args(files);
        self.run_step(Step::Analyse, &args, cancel, on_item)
    }

    /// Elaborates the design unit `top`, optionally overriding generics.
    pub fn elaborate(
        &self,
        top: &str,
        generics: &[(String, String)],
        cancel: &CancelToken,
        on_item: &mut dyn FnMut(&OutputItem),
    ) -> std::io::Result<StepReport> {
        let args = self.options.elaborate_args(top, generics);
        self.run_step(Step::Elaborate, &args, cancel, on_item)
    }

    /// Runs the already elaborated `top`.
    pub fn run(
        &self,
        top: &str,
        run: &RunOptions,
        cancel: &CancelToken,
        on_item: &mut dyn FnMut(&OutputItem),
    ) -> std::io::Result<StepReport> {
        let args = self.options.run_args(top, run);
        self.run_step(Step::Run, &args, cancel, on_item)
    }

    fn run_step(
        &self,
        step: Step,
        args: &[String],
        cancel: &CancelToken,
        on_item: &mut dyn FnMut(&OutputItem),
    ) -> std::io::Result<StepReport> {
        let mut items = Vec::new();
        let mut keep = |item: OutputItem| {
            on_item(&item);
            items.push(item);
        };

        // Analyse and elaborate print multi-line "full" diagnostics; run
        // prints one-line "compact" messages (see options.rs for why).
        let mut diagnostic_parser = DiagnosticParser::new();
        let process = run_process(
            &self.program,
            args,
            &self.working_dir,
            cancel,
            &mut |stream, line| match (stream, step) {
                // nvc doesn't normally write to stdout; keep it as text.
                (Stream::Stdout, _) => keep(OutputItem::Text(line.to_string())),
                (Stream::Stderr, Step::Run) => keep(parse_compact_line(line)),
                (Stream::Stderr, _) => {
                    for item in diagnostic_parser.push_line(line) {
                        keep(item);
                    }
                }
            },
        )?;
        if let Some(item) = diagnostic_parser.finish() {
            keep(item);
        }

        Ok(StepReport {
            step,
            process,
            items,
        })
    }

    /// nvc creates the library folder itself but not its parent, so
    /// `--work=work:build/work` fails unless `build/` exists.
    fn create_work_library_parent(&self) -> std::io::Result<()> {
        let library_dir = self.working_dir.join(&self.options.work.dir);
        match library_dir.parent() {
            Some(parent) if parent != Path::new("") => std::fs::create_dir_all(parent),
            _ => Ok(()),
        }
    }
}
