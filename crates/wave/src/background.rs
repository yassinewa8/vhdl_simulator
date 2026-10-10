//! Opening waveforms and loading signals on a background thread, so the
//! window stays responsive. Same pattern as the GUI's build worker: a
//! thread does the work and sends results through a channel; the window
//! picks them up each frame. `wake` asks the window to redraw.
//!
//! Stop works between signals: `wellen` can't interrupt the reading of a
//! single signal, which takes under a second even for 10 million changes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

use studio_core::CancelToken;
use wellen::{Signal, SignalRef};

use crate::waveform::{Shared, SignalId, WaveError, Waveform};

/// Reads `wanted` signals one at a time, stopping early if cancelled.
pub(crate) fn load_signals(shared: &Shared, wanted: &[SignalRef], cancel: &CancelToken, on_loaded: &mut dyn FnMut(Signal)) {
    for &signal_ref in wanted {
        if cancel.is_cancelled() {
            return;
        }
        let signal = {
            // A panic in another load would poison the lock; the reader
            // itself is still fine to use.
            let mut source = shared.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            source.load_signals(&[signal_ref], &shared.hierarchy, true).remove(0)
        };
        on_loaded(signal);
    }
}

/// How far opening a file has got, shared with the opening thread.
#[derive(Clone, Default)]
pub(crate) struct OpenProgress {
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

impl OpenProgress {
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// The counter `wellen` adds to as it reads.
    pub fn counter(&self) -> Arc<AtomicU64> {
        self.done.clone()
    }

    fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0))
    }
}

/// A file being opened in the background.
pub struct BackgroundOpen {
    path: PathBuf,
    receiver: mpsc::Receiver<Result<Waveform, WaveError>>,
    progress: OpenProgress,
}

/// Starts opening `path` on a new thread.
pub fn open_in_background(path: &Path, wake: impl Fn() + Send + 'static) -> BackgroundOpen {
    let (sender, receiver) = mpsc::channel();
    let progress = OpenProgress::default();
    let thread_progress = progress.clone();
    let thread_path = path.to_path_buf();
    thread::spawn(move || {
        let result = Waveform::open_with_progress(&thread_path, Some(&thread_progress));
        let _ = sender.send(result); // the window may have given up waiting
        wake();
    });
    BackgroundOpen {
        path: path.to_path_buf(),
        receiver,
        progress,
    }
}

impl BackgroundOpen {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The result, once it's ready. Returns `None` while still opening.
    pub fn poll(&self) -> Option<Result<Waveform, WaveError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(WaveError {
                path: self.path.clone(),
                message: "the opening thread stopped unexpectedly".to_string(),
            })),
        }
    }

    /// 0.0 to 1.0 for files that report progress (VCD), else `None`.
    pub fn progress(&self) -> Option<f32> {
        self.progress.fraction()
    }
}

enum LoadMessage {
    Loaded(Signal),
    Finished,
}

/// Signals being loaded in the background. Pass the results to the
/// waveform with [`BackgroundLoad::receive_into`].
pub struct BackgroundLoad {
    receiver: mpsc::Receiver<LoadMessage>,
    cancel: CancelToken,
    total: usize,
    received: usize,
    finished: bool,
}

impl Waveform {
    /// Starts loading `ids` on a new thread. Signals already loaded are
    /// skipped. The waveform stays usable meanwhile: signals that are
    /// already loaded can be queried as normal.
    pub fn load_in_background(&self, ids: &[SignalId], wake: impl Fn() + Send + 'static) -> BackgroundLoad {
        let wanted = self.not_yet_loaded(ids);
        let total = wanted.len();
        let shared = self.shared().clone();
        let cancel = CancelToken::new();
        let thread_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            load_signals(&shared, &wanted, &thread_cancel, &mut |signal| {
                let _ = sender.send(LoadMessage::Loaded(signal));
                wake();
            });
            let _ = sender.send(LoadMessage::Finished);
            wake();
        });
        BackgroundLoad {
            receiver,
            cancel,
            total,
            received: 0,
            finished: false,
        }
    }
}

impl BackgroundLoad {
    /// Moves signals that have arrived into `waveform`. Returns how many.
    pub fn receive_into(&mut self, waveform: &mut Waveform) -> usize {
        let mut count = 0;
        for message in self.receiver.try_iter() {
            match message {
                LoadMessage::Loaded(signal) => {
                    waveform.insert(signal);
                    count += 1;
                }
                LoadMessage::Finished => self.finished = true,
            }
        }
        self.received += count;
        count
    }

    /// True once every signal has arrived, or loading was stopped.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// (signals received, signals requested).
    pub fn progress(&self) -> (usize, usize) {
        (self.received, self.total)
    }

    /// Stops after the signal currently being read.
    pub fn stop(&self) {
        self.cancel.cancel();
    }
}
