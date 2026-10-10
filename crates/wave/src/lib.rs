//! Waveform loading and querying. No GUI code: the app crate draws, this
//! crate only answers questions like "what was this signal at time t".
//!
//! Files are read with the `wellen` crate (also used by the Surfer
//! viewer). Only this crate uses it: everything it returns is our own
//! type, so `wellen` could be replaced without touching anything else.
//!
//! Modules:
//! - `waveform`: an open file, its tree, loaded signals, values
//! - `query`: what the viewer asks each frame (changes in a window,
//!   per-pixel summary, next/previous change or value)
//! - `background`: opening and loading on a thread, with progress and Stop
//! - `convert`: turning `wellen`'s types into ours
//!
//! Typical use:
//!
//! ```no_run
//! use studio_core::{CancelToken, SimTime};
//! use wave::Waveform;
//!
//! let mut wave = Waveform::open("build/waves/counter_tb.fst".as_ref())?;
//! let clk = wave.find_signal("counter_tb.clk").expect("no such signal");
//! wave.load(&[clk], &CancelToken::new());
//! let value = wave.value_at(clk, SimTime::parse("15 ns").unwrap());
//! # Ok::<(), wave::WaveError>(())
//! ```

mod background;
mod convert;
mod query;
mod waveform;

pub use background::{BackgroundLoad, BackgroundOpen, open_in_background};
pub use convert::ScopeKind;
pub use query::{Bucket, Change, Changes, Summary};
pub use waveform::{ScopeId, ScopeInfo, SignalId, SignalInfo, SignalKind, WaveError, Waveform};

/// Waveform file extensions we can open.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["fst", "vcd", "ghw"];
