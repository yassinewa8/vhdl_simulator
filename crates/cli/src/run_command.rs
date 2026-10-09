//! `cli run`: analyse, elaborate and run a testbench, printing the results.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nvc::{CancelToken, Library, Nvc, NvcOptions, RunOptions, StepReport, VhdlStandard, locate_nvc};
use studio_core::SimTime;

use crate::format::format_item;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// VHDL source files, in compile order.
    #[arg(required = true)]
    pub files: Vec<PathBuf>,

    /// Top-level entity to elaborate and run (usually the testbench).
    #[arg(long)]
    pub top: String,

    /// VHDL standard: 1993, 2000, 2002, 2008 or 2019.
    #[arg(long = "std", default_value = "2008", value_parser = parse_standard)]
    pub standard: VhdlStandard,

    /// Stop the simulation at this time, e.g. 10us.
    #[arg(long, value_parser = parse_time)]
    pub stop_time: Option<SimTime>,

    /// Set a top-level generic, e.g. -g WIDTH=16. Can be repeated.
    #[arg(short = 'g', long = "generic", value_parser = parse_generic)]
    pub generics: Vec<(String, String)>,

    /// Folder for the compiled work library.
    #[arg(long, default_value = "work")]
    pub work: PathBuf,

    /// Write an FST waveform to this file.
    #[arg(long)]
    pub wave: Option<PathBuf>,

    /// Path to the nvc program. Default: search PATH.
    #[arg(long)]
    pub nvc: Option<PathBuf>,
}

pub fn run(args: RunArgs) -> ExitCode {
    let install = match locate_nvc(args.nvc.as_deref()) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Using nvc {} ({})", install.version, install.program.display());

    let options = NvcOptions {
        standard: args.standard,
        work: Library {
            name: "work".to_string(),
            dir: args.work.clone(),
        },
    };
    let nvc = Nvc::new(&install, options, ".");
    // Ctrl+C stops nvc too (the terminal sends it to both programs), so
    // the CLI never needs to cancel; the GUI's Stop button will.
    let cancel = CancelToken::new();
    let mut print = |item: &nvc::OutputItem| println!("{}", format_item(item));

    let run_options = RunOptions {
        stop_time: args.stop_time,
        wave_file: args.wave.clone(),
    };

    // Each step only runs if the previous one succeeded.
    let passed = step_succeeded(nvc.analyse(&args.files, &cancel, &mut print))
        && step_succeeded(nvc.elaborate(&args.top, &args.generics, &cancel, &mut print))
        && step_succeeded(nvc.run(&args.top, &run_options, &cancel, &mut print));

    if passed {
        println!("PASSED");
        ExitCode::SUCCESS
    } else {
        println!("FAILED");
        ExitCode::FAILURE
    }
}

/// Prints a one-line summary of a finished step and says whether it passed.
fn step_succeeded(result: std::io::Result<StepReport>) -> bool {
    match result {
        Ok(report) => {
            println!("{}", summary_line(&report));
            report.succeeded()
        }
        Err(error) => {
            eprintln!("error: could not start nvc: {error}");
            false
        }
    }
}

/// e.g. `-- analyse: ok (0.12 s)` or `-- run: failed, 2 errors (1.50 s)`.
fn summary_line(report: &StepReport) -> String {
    let seconds = report.process.duration.as_secs_f64();
    let outcome = if report.process.cancelled {
        "cancelled".to_string()
    } else if report.succeeded() {
        "ok".to_string()
    } else {
        match report.error_count() {
            1 => "failed, 1 error".to_string(),
            count => format!("failed, {count} errors"),
        }
    };
    format!("-- {}: {outcome} ({seconds:.2} s)", report.step)
}

fn parse_standard(text: &str) -> Result<VhdlStandard, String> {
    VhdlStandard::parse(text).ok_or_else(|| format!("unknown VHDL standard '{text}'"))
}

fn parse_time(text: &str) -> Result<SimTime, String> {
    SimTime::parse(text)
        .ok_or_else(|| format!("'{text}' is not a time; use a whole number and unit, e.g. 10us"))
}

fn parse_generic(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((name, value)) if !name.is_empty() => Ok((name.to_string(), value.to_string())),
        _ => Err(format!("'{text}' should look like NAME=VALUE")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvc::{ProcessResult, Step};
    use std::time::Duration;

    fn report(exit_code: Option<i32>, cancelled: bool) -> StepReport {
        StepReport {
            step: Step::Run,
            process: ProcessResult {
                exit_code,
                cancelled,
                duration: Duration::from_millis(1500),
            },
            items: vec![],
        }
    }

    #[test]
    fn summary_lines() {
        assert_eq!(summary_line(&report(Some(0), false)), "-- run: ok (1.50 s)");
        assert_eq!(summary_line(&report(Some(1), false)), "-- run: failed, 0 errors (1.50 s)");
        assert_eq!(summary_line(&report(None, true)), "-- run: cancelled (1.50 s)");
    }

    #[test]
    fn parses_generics() {
        assert_eq!(parse_generic("WIDTH=16"), Ok(("WIDTH".to_string(), "16".to_string())));
        assert_eq!(parse_generic("NAME="), Ok(("NAME".to_string(), String::new())));
        assert!(parse_generic("WIDTH").is_err());
        assert!(parse_generic("=16").is_err());
    }

    #[test]
    fn parses_times_and_standards() {
        assert_eq!(parse_time("10us"), Ok(SimTime::parse("10us").unwrap()));
        assert!(parse_time("10").is_err());
        assert_eq!(parse_standard("93"), Ok(VhdlStandard::Vhdl1993));
        assert!(parse_standard("2010").is_err());
    }
}
