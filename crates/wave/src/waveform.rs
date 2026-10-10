//! An open waveform file: its tree of scopes and signals, and the values
//! of the signals loaded so far.
//!
//! Opening reads only the tree and the list of change times. A signal's
//! values are read from the file when it's first needed (`load`), because
//! a big dump can hold far more data than ever gets displayed.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use studio_core::{CancelToken, SimTime, Value};
use wellen::{Hierarchy, LoadOptions, ScopeRef, Signal, SignalEncoding, SignalRef, SignalSource, SignalValueRef, VarRef, viewers};

use crate::convert::{ScopeKind, TickScale, femtoseconds_per_tick, to_value, value_matches};

/// A scope (instance, block, process, record...) in the waveform tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub(crate) usize);

/// A signal in the waveform tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(pub(crate) usize);

impl SignalId {
    /// An id from its number. Ids are only meaningful for the waveform
    /// they came from; this is for tests that need ids without a file.
    pub fn from_index(index: usize) -> SignalId {
        SignalId(index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeInfo {
    pub id: ScopeId,
    pub name: String,
    /// Dotted path from the top, e.g. `types_tb.nested`.
    pub path: String,
    pub kind: ScopeKind,
    pub scopes: Vec<ScopeId>,
    pub signals: Vec<SignalId>,
}

/// How a signal's values are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// std_logic-style bits: [`Value::Logic`].
    Logic { width: u32 },
    /// Enumerations, booleans, time: [`Value::Text`].
    Text,
    /// [`Value::Real`].
    Real,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalInfo {
    pub id: SignalId,
    pub name: String,
    /// Dotted path from the top, e.g. `types_tb.rec.valid`.
    pub path: String,
    /// The VHDL type name as nvc recorded it, e.g. `SIGNED`, `STATE_T`.
    pub vhdl_type: Option<String>,
    pub kind: SignalKind,
}

/// Why a waveform couldn't be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for WaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for WaveError {}

/// The parts shared with background loaders.
///
/// The file reader (`source`) sits behind a mutex so only one load reads
/// the file at a time. Everything else never changes after opening.
#[derive(Clone)]
pub(crate) struct Shared {
    pub hierarchy: Arc<Hierarchy>,
    pub source: Arc<Mutex<SignalSource>>,
}

/// An open waveform.
pub struct Waveform {
    path: PathBuf,
    shared: Shared,
    /// Every time at which something changes, in file ticks, ascending.
    time_table: Arc<Vec<u64>>,
    scale: TickScale,
    /// Signals read from the file so far. Several tree entries can share
    /// one signal (e.g. a port and the signal connected to it).
    loaded: HashMap<SignalRef, Signal>,
}

impl Waveform {
    /// Opens an FST, VCD or GHW file, reading its tree and change times.
    pub fn open(path: &Path) -> Result<Waveform, WaveError> {
        Waveform::open_with_progress(path, None)
    }

    /// Like [`Waveform::open`], updating `progress` as a VCD is read (FST
    /// files open almost instantly and report nothing).
    pub(crate) fn open_with_progress(path: &Path, progress: Option<&crate::background::OpenProgress>) -> Result<Waveform, WaveError> {
        let error = |message: String| WaveError {
            path: path.to_path_buf(),
            message,
        };
        if !path.is_file() {
            return Err(error("file not found".to_string()));
        }
        let header = viewers::read_header_from_file(path, &LoadOptions::default())
            .map_err(|e| error(explain_load_error(&e.to_string())))?;
        let hierarchy = header.hierarchy;
        if let Some(progress) = progress {
            progress.set_total(header.body_len);
        }
        let body = viewers::read_body(header.body, &hierarchy, progress.map(|p| p.counter()))
            .map_err(|e| error(e.to_string()))?;

        Ok(Waveform {
            path: path.to_path_buf(),
            scale: TickScale {
                femtoseconds_per_tick: femtoseconds_per_tick(hierarchy.timescale()),
            },
            shared: Shared {
                hierarchy: Arc::new(hierarchy),
                source: Arc::new(Mutex::new(body.source)),
            },
            time_table: Arc::new(body.time_table),
            loaded: HashMap::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn shared(&self) -> &Shared {
        &self.shared
    }

    fn hierarchy(&self) -> &Hierarchy {
        &self.shared.hierarchy
    }

    /// The last time anything changes.
    pub fn end_time(&self) -> SimTime {
        self.scale.to_time(self.time_table.last().copied().unwrap_or(0))
    }

    /// The scopes at the top of the tree (usually just the testbench).
    pub fn top_scopes(&self) -> Vec<ScopeId> {
        self.hierarchy().scopes().map(|scope| ScopeId(scope.index())).collect()
    }

    pub fn scope(&self, id: ScopeId) -> ScopeInfo {
        let hierarchy = self.hierarchy();
        let scope = &hierarchy[scope_ref(id)];
        ScopeInfo {
            id,
            name: scope.name(hierarchy).to_string(),
            path: scope.full_name(hierarchy),
            kind: ScopeKind::from_wellen(scope.scope_type()),
            scopes: scope.scopes(hierarchy).map(|s| ScopeId(s.index())).collect(),
            signals: scope.vars(hierarchy).map(|v| SignalId(v.index())).collect(),
        }
    }

    pub fn signal(&self, id: SignalId) -> SignalInfo {
        let hierarchy = self.hierarchy();
        let var = &hierarchy[var_ref(id)];
        let kind = match var.signal_encoding(hierarchy) {
            SignalEncoding::BitVector(width) => SignalKind::Logic { width },
            SignalEncoding::Real => SignalKind::Real,
            SignalEncoding::String => SignalKind::Text,
        };
        SignalInfo {
            id,
            name: var.name(hierarchy).to_string(),
            path: var.full_name(hierarchy),
            vhdl_type: var.vhdl_type_name(hierarchy).map(str::to_string),
            kind,
        }
    }

    /// Finds a signal by its dotted path, e.g. `types_tb.rec.valid`.
    /// Case-insensitive, like VHDL names. (A plain search through every
    /// signal; fine for occasional use such as restoring a saved layout.)
    pub fn find_signal(&self, path: &str) -> Option<SignalId> {
        let hierarchy = self.hierarchy();
        hierarchy
            .all_vars()
            .find(|&var| hierarchy[var].full_name(hierarchy).eq_ignore_ascii_case(path))
            .map(|var| SignalId(var.index()))
    }

    /// Every signal in the file, for "add all" style features and tests.
    pub fn all_signals(&self) -> Vec<SignalId> {
        self.hierarchy().all_vars().map(|var| SignalId(var.index())).collect()
    }

    pub fn is_loaded(&self, id: SignalId) -> bool {
        self.loaded.contains_key(&self.signal_ref(id))
    }

    /// Reads signals from the file, on this thread. Signals already
    /// loaded are skipped. Stops between signals if `cancel` is set.
    /// For big files use [`Waveform::load_in_background`] instead.
    pub fn load(&mut self, ids: &[SignalId], cancel: &CancelToken) {
        let wanted = self.not_yet_loaded(ids);
        // A copy of the shared handles (cheap: they're reference-counted),
        // so `self` can be updated as each signal arrives.
        let shared = self.shared.clone();
        crate::background::load_signals(&shared, &wanted, cancel, &mut |signal| self.insert(signal));
    }

    /// The signals among `ids` that still need reading, without repeats.
    pub(crate) fn not_yet_loaded(&self, ids: &[SignalId]) -> Vec<SignalRef> {
        let mut wanted: Vec<SignalRef> = Vec::new();
        for &id in ids {
            let signal = self.signal_ref(id);
            if !self.loaded.contains_key(&signal) && !wanted.contains(&signal) {
                wanted.push(signal);
            }
        }
        wanted
    }

    pub(crate) fn insert(&mut self, signal: Signal) {
        self.loaded.insert(signal.signal_ref(), signal);
    }

    /// The value at `time`: the last change at or before it, after any
    /// delta cycles at that time have settled. `None` if the signal isn't
    /// loaded or hasn't got a value yet.
    pub fn value_at(&self, id: SignalId, time: SimTime) -> Option<Value> {
        let view = self.view(id)?;
        // The last change at or before `time`.
        let change = view.count_up_to(time).checked_sub(1)?;
        Some(view.value(change))
    }

    /// A loaded signal's changes, for the queries in `query.rs`.
    pub(crate) fn view(&self, id: SignalId) -> Option<SignalView<'_>> {
        Some(SignalView {
            signal: self.loaded.get(&self.signal_ref(id))?,
            time_table: &self.time_table,
            scale: self.scale,
            is_vhdl_time: self.is_vhdl_time(id),
        })
    }

    fn signal_ref(&self, id: SignalId) -> SignalRef {
        self.hierarchy()[var_ref(id)].signal_ref()
    }

    fn is_vhdl_time(&self, id: SignalId) -> bool {
        let hierarchy = self.hierarchy();
        hierarchy[var_ref(id)].vhdl_type_name(hierarchy) == Some("TIME")
    }
}

/// One loaded signal's list of changes. Change number `k` is the k-th
/// time the signal changes (several delta-cycle changes at the same time
/// count as one change, to the value they settle on).
pub(crate) struct SignalView<'a> {
    signal: &'a Signal,
    time_table: &'a [u64],
    scale: TickScale,
    is_vhdl_time: bool,
}

impl SignalView<'_> {
    /// How many changes there are.
    pub fn len(&self) -> usize {
        self.signal.time_indices().len()
    }

    pub fn time(&self, change: usize) -> SimTime {
        let index = self.signal.time_indices()[change];
        self.scale.to_time(self.time_table[index as usize])
    }

    /// The value the signal settles on at change number `change`.
    pub fn value(&self, change: usize) -> Value {
        to_value(self.raw_value(change), self.is_vhdl_time)
    }

    /// True if the value at change number `change` is `target` (checked
    /// in place, which is much faster than `value(change) == *target`).
    pub fn value_is(&self, change: usize, target: &Value) -> bool {
        value_matches(self.raw_value(change), target, self.is_vhdl_time)
    }

    fn raw_value(&self, change: usize) -> SignalValueRef<'_> {
        let index = self.signal.time_indices()[change];
        let offset = self.signal.get_offset(index).expect("a change has data");
        // Several changes at the same time are delta cycles; the last
        // one is the value the signal settles on.
        let settled = offset.elements - 1;
        self.signal.get_value_at(&offset, settled)
    }

    /// How many changes happen strictly before `time` (binary search).
    pub fn count_before(&self, time: SimTime) -> usize {
        self.count_where(|change_time| change_time < time)
    }

    /// How many changes happen at or before `time` (binary search).
    pub fn count_up_to(&self, time: SimTime) -> usize {
        self.count_where(|change_time| change_time <= time)
    }

    fn count_where(&self, is_early: impl Fn(SimTime) -> bool) -> usize {
        self.signal
            .time_indices()
            .partition_point(|&index| is_early(self.scale.to_time(self.time_table[index as usize])))
    }
}

/// Adds an explanation to `wellen` errors we know the cause of.
fn explain_load_error(message: &str) -> String {
    // nvc writes the VHDL source path into a VCD unquoted; a space in it
    // splits it into extra words, which the VCD reader rejects.
    if message.contains("command attribute: misc") {
        format!(
            "{message}\nThis VCD was written by nvc from a source folder whose path contains a \
             space, which VCD readers can't handle. Use FST waveforms (the default), or move \
             the project to a folder without spaces."
        )
    } else {
        message.to_string()
    }
}

fn scope_ref(id: ScopeId) -> ScopeRef {
    ScopeRef::from_index(id.0).expect("ScopeIds only come from this waveform")
}

fn var_ref(id: SignalId) -> VarRef {
    VarRef::from_index(id.0).expect("SignalIds only come from this waveform")
}
