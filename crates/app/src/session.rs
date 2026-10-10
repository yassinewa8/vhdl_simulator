//! What the window shows about the current and previous jobs: the output
//! log, the Problems list, and a progress/result line.
//!
//! Plain data, updated from worker messages, with no GUI code, so it can
//! be tested. The window only draws it.

use nvc::{OutputItem, Step, format_item};
use project::BuildEvent;
use studio_core::{Location, Severity};

use crate::worker::{JobOutcome, WorkerMessage};

/// The log keeps at most this many lines, dropping the oldest. A long
/// simulation that reports every cycle would otherwise use up memory.
pub const MAX_LOG_LINES: usize = 50_000;

/// How a log line should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStyle {
    Plain,
    /// Start of a job, e.g. `── Build ──`.
    Heading,
    Note,
    Warning,
    Error,
    Success,
}

impl LogStyle {
    fn for_severity(severity: Option<Severity>) -> LogStyle {
        match severity {
            None => LogStyle::Plain,
            Some(Severity::Note) => LogStyle::Note,
            Some(Severity::Warning) => LogStyle::Warning,
            Some(_) => LogStyle::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub text: String,
    pub style: LogStyle,
}

/// One entry in the Problems list: a warning or error from nvc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    pub message: String,
    pub location: Option<Location>,
    /// nvc's full message, including the source snippet, as printed.
    pub details: String,
}

impl Problem {
    /// A problem for nvc messages that are warnings or worse.
    fn from_item(item: &OutputItem) -> Option<Problem> {
        let problem = match item {
            OutputItem::Diagnostic(diagnostic) => Problem {
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                location: diagnostic.location.clone(),
                details: if diagnostic.raw.is_empty() {
                    format_item(item)
                } else {
                    diagnostic.raw.join("\n")
                },
            },
            OutputItem::SimMessage(message) => Problem {
                severity: message.severity,
                message: format!("[{}] {}", message.time, message.message),
                location: message.location.clone(),
                details: message.raw.clone(),
            },
            OutputItem::Text(_) => return None,
        };
        (problem.severity >= Severity::Warning).then_some(problem)
    }
}

/// Everything shown about jobs.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub log: Vec<LogLine>,
    pub problems: Vec<Problem>,
    /// What's happening right now, e.g. `Compiling 2/6: src/a.vhd`.
    pub progress: Option<String>,
    /// How the last job ended.
    pub last_outcome: Option<JobOutcome>,
}

impl Session {
    /// Call when a job starts. Problems are cleared (they belong to the
    /// latest job), but the log is kept so earlier output can be read.
    pub fn begin_job(&mut self, description: &str) {
        self.problems.clear();
        self.last_outcome = None;
        self.progress = Some(format!("{description}…"));
        if !self.log.is_empty() {
            self.push("", LogStyle::Plain);
        }
        self.push(&format!("── {description} ──"), LogStyle::Heading);
    }

    /// Adds a plain message of our own (not from a job) to the log.
    pub fn note(&mut self, text: &str, style: LogStyle) {
        self.push(text, style);
    }

    pub fn clear_log(&mut self) {
        self.log.clear();
    }

    /// Updates the session from one worker message.
    pub fn handle(&mut self, message: WorkerMessage) {
        match message {
            WorkerMessage::Event(event) => self.handle_event(event),
            WorkerMessage::Line(text) => self.push(&text, LogStyle::Plain),
            WorkerMessage::Finished(outcome) => {
                let style = if outcome.succeeded {
                    LogStyle::Success
                } else if outcome.cancelled {
                    LogStyle::Warning
                } else {
                    LogStyle::Error
                };
                self.push(&outcome.summary, style);
                self.progress = None;
                self.last_outcome = Some(outcome);
            }
        }
    }

    fn handle_event(&mut self, event: BuildEvent) {
        match event {
            BuildEvent::FileStarted {
                path,
                library,
                number,
                total,
            } => {
                self.progress = Some(format!("Compiling {number}/{total}: {}", path.display()));
                self.push(&format!("[{number}/{total}] {library}: {}", path.display()), LogStyle::Plain);
            }
            BuildEvent::FileFinished { process, errors, .. } if !process.succeeded() => {
                let text = if process.cancelled {
                    "    stopped".to_string()
                } else {
                    format!("    failed ({errors} errors)")
                };
                self.push(&text, LogStyle::Error);
            }
            BuildEvent::FileFinished { .. } => {}
            BuildEvent::FileSkipped { path, because } => {
                let text = format!("skipped {}: it needs {}, which failed", path.display(), because.display());
                self.push(&text, LogStyle::Warning);
            }
            BuildEvent::StepStarted { step, top } => {
                let verb = match step {
                    Step::Elaborate => "Elaborating",
                    Step::Run => "Running",
                    Step::Analyse => "Analysing",
                };
                self.progress = Some(format!("{verb} {top}"));
                self.push(&format!("{verb} {top}"), LogStyle::Plain);
            }
            BuildEvent::StepFinished { step, process, .. } => {
                let seconds = process.duration.as_secs_f64();
                let (text, style) = if process.cancelled {
                    (format!("{step} stopped after {seconds:.2} s"), LogStyle::Warning)
                } else if process.succeeded() {
                    (format!("{step} done ({seconds:.2} s)"), LogStyle::Plain)
                } else {
                    (format!("{step} failed ({seconds:.2} s)"), LogStyle::Error)
                };
                self.push(&text, style);
            }
            BuildEvent::Output(item) => {
                let style = LogStyle::for_severity(item.severity());
                for line in format_item(&item).lines() {
                    self.push(line, style);
                }
                if let Some(problem) = Problem::from_item(&item) {
                    self.problems.push(problem);
                }
            }
        }
    }

    fn push(&mut self, text: &str, style: LogStyle) {
        self.log.push(LogLine {
            text: text.to_string(),
            style,
        });
        if self.log.len() > MAX_LOG_LINES {
            // Drop a chunk at once rather than one line per message, so
            // a busy simulation doesn't shift the whole log every line.
            let excess = self.log.len() - MAX_LOG_LINES + MAX_LOG_LINES / 10;
            self.log.drain(..excess);
        }
    }

    /// Number of problems that are errors (as opposed to warnings).
    pub fn error_count(&self) -> usize {
        self.problems.iter().filter(|p| p.severity.is_error()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvc::ProcessResult;
    use std::path::PathBuf;
    use std::time::Duration;
    use studio_core::{Diagnostic, SimMessage, SimTime};

    fn diagnostic(severity: Severity) -> OutputItem {
        OutputItem::Diagnostic(Diagnostic {
            severity,
            message: "no visible declaration for B".to_string(),
            location: Some(Location {
                file: PathBuf::from("src/a.vhd"),
                line: 11,
                column: Some(10),
            }),
            notes: vec!["did you mean A?".to_string()],
            raw: vec!["** Error: no visible declaration for B".to_string(), "    > src/a.vhd:11".to_string()],
        })
    }

    fn texts(session: &Session) -> Vec<&str> {
        session.log.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn errors_become_problems_and_coloured_log_lines() {
        let mut session = Session::default();
        session.begin_job("Build");
        session.handle(WorkerMessage::Event(BuildEvent::Output(diagnostic(Severity::Error))));

        assert_eq!(session.problems.len(), 1);
        let problem = &session.problems[0];
        assert_eq!(problem.message, "no visible declaration for B");
        assert_eq!(problem.details, "** Error: no visible declaration for B\n    > src/a.vhd:11");
        assert_eq!(session.error_count(), 1);

        // Formatted over two lines in the log: message, then the hint.
        assert_eq!(
            texts(&session),
            [
                "── Build ──",
                "src/a.vhd:11:10: error: no visible declaration for B",
                "    = did you mean A?"
            ]
        );
        assert_eq!(session.log[1].style, LogStyle::Error);
    }

    #[test]
    fn notes_are_logged_but_not_problems() {
        let mut session = Session::default();
        let note = OutputItem::SimMessage(SimMessage {
            severity: Severity::Note,
            time: SimTime::parse("10ns").unwrap(),
            delta: 0,
            message: "y = 25".to_string(),
            location: None,
            raw: String::new(),
        });
        session.handle(WorkerMessage::Event(BuildEvent::Output(note)));
        assert!(session.problems.is_empty());
        assert_eq!(session.log[0].style, LogStyle::Note);

        session.handle(WorkerMessage::Event(BuildEvent::Output(diagnostic(Severity::Warning))));
        assert_eq!(session.problems.len(), 1);
        assert_eq!(session.error_count(), 0);
    }

    #[test]
    fn progress_follows_the_job() {
        let mut session = Session::default();
        session.begin_job("Run top_tb");
        assert_eq!(session.progress.as_deref(), Some("Run top_tb…"));

        session.handle(WorkerMessage::Event(BuildEvent::FileStarted {
            path: PathBuf::from("src/a.vhd"),
            library: "work".to_string(),
            number: 1,
            total: 2,
        }));
        assert_eq!(session.progress.as_deref(), Some("Compiling 1/2: src/a.vhd"));

        session.handle(WorkerMessage::Event(BuildEvent::StepStarted {
            step: Step::Run,
            top: "top_tb".to_string(),
        }));
        assert_eq!(session.progress.as_deref(), Some("Running top_tb"));

        let outcome = JobOutcome {
            succeeded: true,
            cancelled: false,
            summary: "top_tb PASSED".to_string(),
            wave_file: None,
        };
        session.handle(WorkerMessage::Finished(outcome.clone()));
        assert_eq!(session.progress, None);
        assert_eq!(session.last_outcome, Some(outcome));
        assert_eq!(session.log.last().unwrap().style, LogStyle::Success);
    }

    #[test]
    fn new_job_clears_problems_but_keeps_the_log() {
        let mut session = Session::default();
        session.begin_job("Build");
        session.handle(WorkerMessage::Event(BuildEvent::Output(diagnostic(Severity::Error))));
        session.begin_job("Build");
        assert!(session.problems.is_empty());
        assert_eq!(texts(&session)[3..], ["", "── Build ──"]);
    }

    #[test]
    fn failed_and_stopped_steps_are_marked() {
        let mut session = Session::default();
        let process = |exit_code, cancelled| ProcessResult {
            exit_code,
            cancelled,
            duration: Duration::from_millis(250),
        };
        session.handle(WorkerMessage::Event(BuildEvent::StepFinished {
            step: Step::Elaborate,
            process: process(Some(1), false),
            errors: 1,
        }));
        session.handle(WorkerMessage::Event(BuildEvent::StepFinished {
            step: Step::Run,
            process: process(None, true),
            errors: 0,
        }));
        assert_eq!(texts(&session), ["elaborate failed (0.25 s)", "run stopped after 0.25 s"]);
        assert_eq!(session.log[0].style, LogStyle::Error);
        assert_eq!(session.log[1].style, LogStyle::Warning);
    }

    #[test]
    fn log_is_capped() {
        let mut session = Session::default();
        for i in 0..MAX_LOG_LINES + 1 {
            session.handle(WorkerMessage::Line(format!("line {i}")));
        }
        assert!(session.log.len() <= MAX_LOG_LINES);
        assert_eq!(session.log.last().unwrap().text, format!("line {MAX_LOG_LINES}"));
    }
}
