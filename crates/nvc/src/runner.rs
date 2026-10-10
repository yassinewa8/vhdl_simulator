//! Running nvc as a separate process, streaming its output, with cancel.
//!
//! How it works, in hardware terms: nvc's stdout and stderr are two FIFOs.
//! A small reader thread drains each one and forwards whole lines into a
//! single queue. The calling thread pops lines off that queue and hands
//! them to the caller's callback, and between lines it checks the cancel
//! flag. If the flag is set, it kills the nvc process.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Which output stream a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// A flag that asks a running nvc process to stop.
///
/// Cloning gives another handle to the *same* flag, so one clone can be
/// given to the runner and another kept by e.g. a Stop button.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> CancelToken {
        CancelToken::default()
    }

    /// Asks the process to stop. Safe to call from any thread.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// How a process finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessResult {
    /// Exit code, or `None` if the process was killed without one.
    pub exit_code: Option<i32>,
    /// True if we killed it because of the cancel flag.
    pub cancelled: bool,
    /// Wall-clock time from start to finish.
    pub duration: Duration,
}

impl ProcessResult {
    /// True if the process ran to completion and reported success.
    pub fn succeeded(&self) -> bool {
        !self.cancelled && self.exit_code == Some(0)
    }
}

/// How often to check the cancel flag while waiting for output.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Runs `program` with `args` in `working_dir` and waits for it to finish.
///
/// Every line of output is passed to `on_line` as soon as it arrives, with
/// the line ending removed. If `cancel` is set while running, the process is
/// killed and the result has `cancelled: true`.
///
/// Returns an error only if the process couldn't be started at all.
pub fn run_process(
    program: &Path,
    args: &[String],
    working_dir: &Path,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(Stream, &str),
) -> std::io::Result<ProcessResult> {
    run_process_with_env(program, args, &[], working_dir, cancel, on_line)
}

/// Like [`run_process`], but also sets environment variables for the
/// process (on top of the ones it inherits from us).
pub fn run_process_with_env(
    program: &Path,
    args: &[String],
    env: &[(String, OsString)],
    working_dir: &Path,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(Stream, &str),
) -> std::io::Result<ProcessResult> {
    let start = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console_window(&mut command);
    let mut child = command.spawn()?;

    let (sender, receiver) = mpsc::channel();
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stdout_reader = spawn_line_reader(stdout, Stream::Stdout, sender.clone());
    let stderr_reader = spawn_line_reader(stderr, Stream::Stderr, sender);

    let mut cancelled = false;
    loop {
        if cancel.is_cancelled() && !cancelled {
            cancelled = true;
            // Ignore the error: it only fails if the process already exited.
            let _ = child.kill();
        }
        match receiver.recv_timeout(CANCEL_POLL_INTERVAL) {
            Ok((stream, line)) => on_line(stream, &line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Both readers have finished: the process closed its output.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait()?;
    // The readers have already finished; joining just tidies up.
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();

    Ok(ProcessResult {
        exit_code: status.code(),
        cancelled,
        duration: start.elapsed(),
    })
}

/// Starts a thread that reads lines from `pipe` and sends them on.
fn spawn_line_reader(
    pipe: impl Read + Send + 'static,
    stream: Stream,
    sender: mpsc::Sender<(Stream, String)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) | Err(_) => break, // end of output, or the pipe broke
                Ok(_) => {
                    // Lossy so odd bytes in a VHDL report can't stop us.
                    let line = String::from_utf8_lossy(&buffer);
                    let line = line.trim_end_matches(['\r', '\n']).to_string();
                    if sender.send((stream, line)).is_err() {
                        break; // nobody is listening any more
                    }
                }
            }
        }
    })
}

/// On Windows, stops a console window flashing up when the GUI runs nvc.
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_token_clones_share_the_flag() {
        let token = CancelToken::new();
        let handle = token.clone();
        assert!(!token.is_cancelled());
        handle.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn succeeded_needs_exit_code_zero_and_no_cancel() {
        let result = |exit_code, cancelled| ProcessResult {
            exit_code,
            cancelled,
            duration: Duration::ZERO,
        };
        assert!(result(Some(0), false).succeeded());
        assert!(!result(Some(1), false).succeeded());
        assert!(!result(None, false).succeeded());
        assert!(!result(Some(0), true).succeeded());
    }

    #[test]
    fn missing_program_is_an_error() {
        let result = run_process(
            Path::new("no_such_program_xyz"),
            &[],
            Path::new("."),
            &CancelToken::new(),
            &mut |_, _| {},
        );
        assert!(result.is_err());
    }
}
