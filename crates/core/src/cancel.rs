//! A Stop flag shared between a running job and the button that stops it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A flag that asks a running job (an nvc process, a waveform load) to stop.
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

    /// Asks the job to stop. Safe to call from any thread.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

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
}
