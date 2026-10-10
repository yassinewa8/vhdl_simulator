//! Running builds, simulations and installs on a background thread.
//!
//! In hardware terms: the window and the worker are two clock domains, and
//! a channel is the FIFO between them. The worker pushes progress messages
//! in; the window pops them once per frame. The window never waits for
//! nvc, so it stays responsive however long a simulation runs.
//!
//! Nothing here uses egui. The worker is told to "wake" the window with a
//! plain callback, so `run_job` can be tested without a GUI.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nvc::{CancelToken, InstallError, NvcInstall, default_tcllib_dir, find_bash, install_package};
use project::{BuildEvent, BuildPlan, Project, TestbenchConfig, plan_build, run_build, run_testbench};

/// Something the worker can do.
#[derive(Debug, Clone)]
pub enum Job {
    /// Compile what changed (or everything, with `force`).
    Build { force: bool },
    /// Build, then elaborate and run a testbench.
    Run { testbench: TestbenchConfig },
    /// Install a library package such as `osvvm`.
    Install { package: String },
}

impl Job {
    /// Short description for the log and status bar.
    pub fn description(&self) -> String {
        match self {
            Job::Build { force: false } => "Build".to_string(),
            Job::Build { force: true } => "Rebuild all".to_string(),
            Job::Run { testbench } => format!("Run {}", testbench.display_name()),
            Job::Install { package } => format!("Install {package}"),
        }
    }
}

/// A message from the worker to the window.
#[derive(Debug, Clone)]
pub enum WorkerMessage {
    /// Build or simulation progress, including nvc's messages.
    Event(BuildEvent),
    /// A plain line of text for the log (e.g. install script output).
    Line(String),
    /// The job is over. Always the last message of a job.
    Finished(JobOutcome),
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobOutcome {
    pub succeeded: bool,
    pub cancelled: bool,
    /// One line for the user, e.g. `Build OK: 3 compiled, 0 up to date`.
    pub summary: String,
}

impl JobOutcome {
    fn failed(summary: impl Into<String>) -> JobOutcome {
        JobOutcome {
            succeeded: false,
            cancelled: false,
            summary: summary.into(),
        }
    }
}

/// What a job needs to run.
#[derive(Debug, Clone)]
pub struct JobContext {
    /// `None` when no project is open (only Install works then).
    pub project: Option<Project>,
    pub nvc: NvcInstall,
}

/// Does `job` on the current thread, sending messages as it goes. The last
/// message sent is always [`WorkerMessage::Finished`].
pub fn run_job(job: &Job, context: &JobContext, cancel: &CancelToken, send: &mut dyn FnMut(WorkerMessage)) {
    let outcome = match job {
        Job::Install { package } => install(package, &context.nvc, cancel, send),
        Job::Build { force } => match &context.project {
            Some(project) => build(project, &context.nvc, *force, cancel, send).0,
            None => JobOutcome::failed("No project is open"),
        },
        Job::Run { testbench } => match &context.project {
            Some(project) => build_and_run(project, &context.nvc, testbench, cancel, send),
            None => JobOutcome::failed("No project is open"),
        },
    };
    send(WorkerMessage::Finished(outcome));
}

/// Builds the project. Also returns the plan if the build succeeded, for
/// running a testbench next.
fn build(
    project: &Project,
    nvc: &NvcInstall,
    force: bool,
    cancel: &CancelToken,
    send: &mut dyn FnMut(WorkerMessage),
) -> (JobOutcome, Option<BuildPlan>) {
    let plan = match plan_build(project, &nvc.version.to_string(), force) {
        Ok(plan) => plan,
        Err(error) => return (JobOutcome::failed(error.to_string()), None),
    };
    for warning in &plan.warnings {
        send(WorkerMessage::Line(format!("warning: {warning}")));
    }
    match run_build(project, &plan, nvc, cancel, &mut |event| send(WorkerMessage::Event(event))) {
        Ok(summary) => {
            let outcome = JobOutcome {
                succeeded: summary.succeeded(),
                cancelled: summary.cancelled,
                summary: summary.to_string(),
            };
            let plan = summary.succeeded().then_some(plan);
            (outcome, plan)
        }
        Err(error) => (JobOutcome::failed(error.to_string()), None),
    }
}

fn build_and_run(
    project: &Project,
    nvc: &NvcInstall,
    testbench: &TestbenchConfig,
    cancel: &CancelToken,
    send: &mut dyn FnMut(WorkerMessage),
) -> JobOutcome {
    let (build_outcome, plan) = build(project, nvc, false, cancel, send);
    let Some(plan) = plan else {
        return build_outcome;
    };
    send(WorkerMessage::Line(build_outcome.summary));

    let name = testbench.display_name();
    match run_testbench(project, &plan, testbench, nvc, cancel, &mut |event| send(WorkerMessage::Event(event))) {
        Ok(outcome) => {
            if let Some(file) = &outcome.wave_file {
                send(WorkerMessage::Line(format!("Waveform: {}", file.display())));
            }
            let cancelled = outcome.reports.iter().any(|report| report.process.cancelled);
            let result = if cancelled {
                "STOPPED"
            } else if outcome.passed() {
                "PASSED"
            } else {
                "FAILED"
            };
            JobOutcome {
                succeeded: outcome.passed(),
                cancelled,
                summary: format!("{name} {result}"),
            }
        }
        Err(error) => JobOutcome::failed(error.to_string()),
    }
}

fn install(package: &str, nvc: &NvcInstall, cancel: &CancelToken, send: &mut dyn FnMut(WorkerMessage)) -> JobOutcome {
    let Some(bash) = find_bash() else {
        return JobOutcome::failed(InstallError::NoBash.to_string());
    };
    send(WorkerMessage::Line(format!("Using {}", bash.program.display())));
    let tcllib = default_tcllib_dir();
    let result = install_package(nvc, &bash, package, tcllib.as_deref(), cancel, &mut |line| {
        send(WorkerMessage::Line(line.to_string()))
    });
    match result {
        Ok(process) => JobOutcome {
            succeeded: process.succeeded(),
            cancelled: process.cancelled,
            summary: if process.succeeded() {
                format!("Installed {package}")
            } else if process.cancelled {
                format!("Install of {package} stopped")
            } else {
                format!("Install of {package} FAILED (see the output above)")
            },
        },
        Err(error) => JobOutcome::failed(error.to_string()),
    }
}

/// A job running on a background thread.
pub struct RunningJob {
    pub description: String,
    receiver: mpsc::Receiver<WorkerMessage>,
    cancel: CancelToken,
}

impl RunningJob {
    /// Starts `job` on a new thread. `wake` is called after every message
    /// so the window redraws promptly (egui only redraws when asked).
    pub fn start(job: Job, context: JobContext, wake: impl Fn() + Send + 'static) -> RunningJob {
        let (sender, receiver) = mpsc::channel();
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        let description = job.description();

        thread::spawn(move || {
            run_job(&job, &context, &worker_cancel, &mut |message| {
                // If the window has gone, nobody is listening: that's fine.
                let _ = sender.send(message);
                wake();
            });
        });
        RunningJob {
            description,
            receiver,
            cancel,
        }
    }

    /// Asks the job to stop (kills the running nvc or install).
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// Stops the job and waits up to `timeout` for it to finish. Used when
    /// the window closes, so no nvc process is left running behind it.
    pub fn stop_and_wait(&self, timeout: Duration) {
        self.stop();
        let deadline = Instant::now() + timeout;
        while let Some(time_left) = deadline.checked_duration_since(Instant::now()) {
            match self.receiver.recv_timeout(time_left) {
                Ok(WorkerMessage::Finished(_)) | Err(_) => return,
                Ok(_) => {}
            }
        }
    }

    pub fn is_stopping(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Every message that has arrived since last time, without waiting.
    pub fn take_messages(&self) -> Vec<WorkerMessage> {
        self.receiver.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_nvc() -> NvcInstall {
        NvcInstall {
            program: PathBuf::from("nvc"),
            version: nvc::MINIMUM_VERSION,
        }
    }

    fn collect(job: &Job, context: &JobContext) -> Vec<WorkerMessage> {
        let mut messages = Vec::new();
        run_job(job, context, &CancelToken::new(), &mut |m| messages.push(m));
        messages
    }

    #[test]
    fn descriptions() {
        assert_eq!(Job::Build { force: false }.description(), "Build");
        assert_eq!(Job::Build { force: true }.description(), "Rebuild all");
        let testbench = TestbenchConfig::new("top_tb");
        assert_eq!(Job::Run { testbench }.description(), "Run top_tb");
        let install = Job::Install {
            package: "osvvm".to_string(),
        };
        assert_eq!(install.description(), "Install osvvm");
    }

    #[test]
    fn build_without_a_project_fails_politely() {
        let context = JobContext {
            project: None,
            nvc: fake_nvc(),
        };
        let messages = collect(&Job::Build { force: false }, &context);
        let [WorkerMessage::Finished(outcome)] = messages.as_slice() else {
            panic!("{messages:?}");
        };
        assert_eq!(*outcome, JobOutcome::failed("No project is open"));
    }

    #[test]
    fn background_job_sends_finished_last_and_wakes_the_window() {
        let context = JobContext {
            project: None,
            nvc: fake_nvc(),
        };
        let (woken_sender, woken) = mpsc::channel();
        let job = RunningJob::start(Job::Build { force: false }, context, move || {
            let _ = woken_sender.send(());
        });
        assert_eq!(job.description, "Build");

        // Wait for the wake-up, as the window would.
        woken.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let messages = job.take_messages();
        assert!(matches!(messages.last(), Some(WorkerMessage::Finished(_))), "{messages:?}");
    }
}
