//! `cli build`: compile a project from its `project.toml`, in dependency
//! order, recompiling only what changed.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nvc::{CancelToken, locate_nvc};
use project::{BuildEvent, BuildPlan, Project, plan_build, run_build};

use nvc::format_item;

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// The project file, or the folder containing project.toml.
    #[arg(default_value = ".")]
    pub project: PathBuf,

    /// Compile every file, ignoring what was compiled before.
    #[arg(long)]
    pub force: bool,

    /// Only show the compile order and what would be compiled.
    #[arg(long)]
    pub dry_run: bool,

    /// Path to the nvc program. Default: search PATH.
    #[arg(long)]
    pub nvc: Option<PathBuf>,
}

pub fn run(args: BuildArgs) -> ExitCode {
    let project = match Project::load(&args.project) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Project {} ({})", project.config.project.name, project.file.display());

    let install = match locate_nvc(args.nvc.as_deref()) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Using nvc {} ({})", install.version, install.program.display());

    let plan = match plan_build(&project, &install.version.to_string(), args.force) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    for warning in &plan.warnings {
        println!("warning: {warning}");
    }

    if args.dry_run {
        print!("{}", describe_plan(&plan));
        return ExitCode::SUCCESS;
    }

    // As with `cli run`, Ctrl+C reaches nvc directly, so no cancelling here.
    let mut print = |event: BuildEvent| {
        if let Some(text) = format_event(&event) {
            println!("{text}");
        }
    };
    match run_build(&project, &plan, &install, &CancelToken::new(), &mut print) {
        Ok(summary) => {
            println!("{summary}");
            if summary.succeeded() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The compile order, one numbered line per file, e.g.
/// `  2. util_lib    util/math_pkg.vhd  (up to date)`.
fn describe_plan(plan: &BuildPlan) -> String {
    let mut text = format!(
        "Compile order ({} files, {} to compile):\n",
        plan.files.len(),
        plan.compile_count()
    );
    let library_width = plan.files.iter().map(|f| f.source.library.len()).max().unwrap_or(0);
    for (index, file) in plan.files.iter().enumerate() {
        let status = if file.needs_compile { "" } else { "  (up to date)" };
        text.push_str(&format!(
            "{:>4}. {:library_width$}  {}{status}\n",
            index + 1,
            file.source.library,
            file.source.path.display()
        ));
    }
    text
}

/// One build event as text, or `None` for events not worth printing.
fn format_event(event: &BuildEvent) -> Option<String> {
    match event {
        BuildEvent::FileStarted {
            path,
            library,
            number,
            total,
        } => Some(format!("[{number}/{total}] {library}: {}", path.display())),
        BuildEvent::FileFinished { process, errors, .. } if !process.succeeded() => {
            Some(if process.cancelled {
                "    cancelled".to_string()
            } else {
                format!("    failed ({})", plural(*errors, "error"))
            })
        }
        BuildEvent::FileFinished { .. } => None,
        BuildEvent::FileSkipped { path, because } => Some(format!(
            "skipped {}: it needs {}, which failed",
            path.display(),
            because.display()
        )),
        BuildEvent::Output(item) => Some(format_item(item)),
        // Only produced when running testbenches, which `cli build` doesn't.
        BuildEvent::StepStarted { .. } | BuildEvent::StepFinished { .. } => None,
    }
}

/// `1 error`, `2 errors`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvc::ProcessResult;
    use std::time::Duration;

    fn process(exit_code: i32) -> ProcessResult {
        ProcessResult {
            exit_code: Some(exit_code),
            cancelled: false,
            duration: Duration::ZERO,
        }
    }

    #[test]
    fn formats_progress_events() {
        let started = BuildEvent::FileStarted {
            path: PathBuf::from("src/a.vhd"),
            library: "design".to_string(),
            number: 2,
            total: 5,
        };
        assert_eq!(format_event(&started).unwrap(), "[2/5] design: src/a.vhd");

        let ok = BuildEvent::FileFinished {
            path: PathBuf::from("src/a.vhd"),
            process: process(0),
            errors: 0,
        };
        assert_eq!(format_event(&ok), None);

        let failed = BuildEvent::FileFinished {
            path: PathBuf::from("src/a.vhd"),
            process: process(1),
            errors: 2,
        };
        assert_eq!(format_event(&failed).unwrap(), "    failed (2 errors)");

        let one_error = BuildEvent::FileFinished {
            path: PathBuf::from("src/a.vhd"),
            process: process(1),
            errors: 1,
        };
        assert_eq!(format_event(&one_error).unwrap(), "    failed (1 error)");

        let skipped = BuildEvent::FileSkipped {
            path: PathBuf::from("tb.vhd"),
            because: PathBuf::from("src/a.vhd"),
        };
        assert_eq!(
            format_event(&skipped).unwrap(),
            "skipped tb.vhd: it needs src/a.vhd, which failed"
        );
    }
}
