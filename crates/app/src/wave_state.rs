//! What the Waves tab shows: the open waveform, the signals added to the
//! view, and the visible time range.
//!
//! No egui here: opening and loading happen on background threads (from
//! the wave crate) and this only keeps track of them, so it can be tested.

use std::path::{Path, PathBuf};

use studio_core::{Radix, SimTime, Value, default_radix, parse_logic_value};
use wave::{BackgroundLoad, BackgroundOpen, SignalId, SignalKind, Waveform, open_in_background};

use crate::cursors::{CursorLine, Cursors};
use crate::timeline::Timeline;

/// A signal in the wave view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayedSignal {
    pub id: SignalId,
    /// Full dotted path, e.g. `top_tb.dut.count`.
    pub path: String,
    /// Shown in the names column: the last part of the path.
    pub label: String,
    pub radix: Radix,
    /// True for one-bit logic signals, drawn as a line rather than a bus.
    pub is_bit: bool,
}

#[derive(Default)]
pub struct WaveState {
    /// A file being opened in the background.
    opening: Option<BackgroundOpen>,
    pub wave: Option<Waveform>,
    /// Why the last open failed, to show in the Waves tab.
    pub error: Option<String>,
    pub signals: Vec<DisplayedSignal>,
    /// Signals being read from the file in the background.
    loads: Vec<BackgroundLoad>,
    pub timeline: Timeline,
    /// Set when a new waveform arrives: the view should zoom to fit once
    /// it knows how wide it is.
    pub needs_fit: bool,
    /// Index into `signals` of the selected row.
    pub selected: Option<usize>,
    /// How far the signal list is scrolled down, in pixels.
    pub row_scroll: f32,
    /// Zoom asked for by a toolbar button, applied around the centre on
    /// the next frame (the button doesn't know the wave area's size).
    pub zoom_request: Option<f64>,
    /// The time under the mouse, shown in the toolbar, already formatted
    /// to the precision of the current zoom.
    pub hover_time: Option<String>,
    /// While hovering near an edge: where a click would put the cursor,
    /// and the row of the signal whose edge it is.
    pub hover_snap: Option<(SimTime, usize)>,
    pub cursors: Cursors,
    /// A time to centre the view on next frame (e.g. "go to marker").
    pub centre_request: Option<SimTime>,
    /// A time to bring into view next frame, if it's off-screen (e.g.
    /// after jumping to the next edge).
    pub reveal_request: Option<SimTime>,
    /// The cursor or marker being dragged with the mouse.
    pub dragging: Option<CursorLine>,
    /// What the "find value" box holds.
    pub find_text: String,
    /// Why the last edge step or find didn't move the cursor.
    pub find_message: Option<String>,
    /// What the search box above the signal tree holds.
    pub tree_filter: String,
    /// The signals matching `tree_filter`, worked out once per filter text.
    filter_cache: Option<(String, Vec<SignalId>)>,
}

/// The search box shows at most this many matches.
pub const MAX_FILTER_RESULTS: usize = 500;

impl WaveState {
    /// Starts opening `path`, replacing whatever was shown. (Keeping the
    /// signal list across re-runs comes in Phase 4 step 6.)
    pub fn open(&mut self, path: &Path, wake: impl Fn() + Send + 'static) {
        self.stop_loading();
        self.opening = Some(open_in_background(path, wake));
        self.wave = None;
        self.error = None;
        self.signals.clear();
        self.selected = None;
        self.row_scroll = 0.0;
        self.cursors = Cursors::default();
        self.dragging = None;
        self.find_message = None;
        self.filter_cache = None;
    }

    /// The file being opened or shown.
    pub fn path(&self) -> Option<PathBuf> {
        match (&self.opening, &self.wave) {
            (Some(opening), _) => Some(opening.path().to_path_buf()),
            (None, Some(wave)) => Some(wave.path().to_path_buf()),
            (None, None) => None,
        }
    }

    pub fn is_opening(&self) -> bool {
        self.opening.is_some()
    }

    /// Takes in background results. Call once per frame. Returns true if
    /// anything changed, so the window knows to redraw.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(result) = self.opening.as_ref().and_then(BackgroundOpen::poll) {
            self.opening = None;
            match result {
                Ok(wave) => {
                    self.wave = Some(wave);
                    self.needs_fit = true;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            changed = true;
        }
        if let Some(wave) = &mut self.wave {
            for load in &mut self.loads {
                changed |= load.receive_into(wave) > 0;
            }
            self.loads.retain(|load| !load.is_finished());
        }
        changed
    }

    /// Adds signals to the view (skipping ones already shown) and starts
    /// reading their data from the file.
    pub fn add(&mut self, ids: &[SignalId], wake: impl Fn() + Send + 'static) {
        let Some(wave) = &self.wave else { return };
        let mut added = Vec::new();
        for &id in ids {
            if self.signals.iter().any(|shown| shown.id == id) {
                continue;
            }
            let info = wave.signal(id);
            let (width, is_bit) = match info.kind {
                SignalKind::Logic { width } => (width as usize, width == 1),
                SignalKind::Text | SignalKind::Real => (0, false),
            };
            self.signals.push(DisplayedSignal {
                id,
                label: info.name.clone(),
                path: info.path,
                radix: default_radix(info.vhdl_type.as_deref(), width),
                is_bit,
            });
            added.push(id);
        }
        if !added.is_empty() {
            self.loads.push(wave.load_in_background(&added, wake));
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.signals.len() {
            self.signals.remove(index);
            self.selected = None;
        }
    }

    /// (signals received, signals requested) across loads in progress,
    /// or `None` when nothing is loading.
    pub fn loading_progress(&self) -> Option<(usize, usize)> {
        if self.loads.is_empty() {
            return None;
        }
        Some(self.loads.iter().fold((0, 0), |(done, total), load| {
            let (d, t) = load.progress();
            (done + d, total + t)
        }))
    }

    /// Moves the main cursor to the selected signal's next (or previous)
    /// change after (or before) the cursor.
    pub fn step_edge(&mut self, forward: bool) -> Result<SimTime, String> {
        let (wave, id) = self.selected_signal()?;
        let found = if forward {
            let from = self.cursors.primary.unwrap_or(SimTime::ZERO);
            wave.next_change(id, from)
        } else {
            let from = self.cursors.primary.unwrap_or(SimTime::from_femtoseconds(u64::MAX));
            wave.prev_change(id, from)
        };
        self.move_cursor(found, if forward { "after" } else { "before" }, "edge")
    }

    /// Moves the main cursor to the next (or previous) time the selected
    /// signal becomes the value typed in `find_text`.
    pub fn find_value(&mut self, forward: bool) -> Result<SimTime, String> {
        let (wave, id) = self.selected_signal()?;
        let index = self.selected.expect("selected_signal checked it");
        let target = target_value(wave, id, self.signals[index].radix, &self.find_text)?;
        let found = if forward {
            wave.next_equal(id, self.cursors.primary.unwrap_or(SimTime::ZERO), &target)
        } else {
            let from = self.cursors.primary.unwrap_or(SimTime::from_femtoseconds(u64::MAX));
            wave.prev_equal(id, from, &target)
        };
        let what = format!("'{}'", self.find_text.trim());
        self.move_cursor(found, if forward { "after" } else { "before" }, &what)
    }

    /// The signals whose name contains `tree_filter` (ignoring case).
    pub fn filtered_signals(&mut self) -> &[SignalId] {
        let filter = self.tree_filter.trim().to_lowercase();
        let up_to_date = self.filter_cache.as_ref().is_some_and(|(cached, _)| *cached == filter);
        if !up_to_date {
            let matches = match &self.wave {
                Some(wave) => wave
                    .all_signals()
                    .into_iter()
                    .filter(|&id| wave.signal(id).name.to_lowercase().contains(&filter))
                    .take(MAX_FILTER_RESULTS)
                    .collect(),
                None => Vec::new(),
            };
            self.filter_cache = Some((filter, matches));
        }
        &self.filter_cache.as_ref().expect("just filled").1
    }

    fn selected_signal(&self) -> Result<(&Waveform, SignalId), String> {
        let wave = self.wave.as_ref().ok_or("no waveform open")?;
        let index = self.selected.ok_or("select a signal first (click its row)")?;
        Ok((wave, self.signals[index].id))
    }

    /// Puts the main cursor on `found` and brings it into view, or
    /// explains why nothing was found.
    fn move_cursor(&mut self, found: Option<SimTime>, direction: &str, what: &str) -> Result<SimTime, String> {
        match found {
            Some(time) => {
                self.cursors.primary = Some(time);
                self.reveal_request = Some(time);
                self.find_message = None;
                Ok(time)
            }
            None => {
                let from = match self.cursors.primary {
                    Some(time) => format!(" {direction} {time}"),
                    None => String::new(),
                };
                let message = format!("no {what}{from}");
                self.find_message = Some(message.clone());
                Err(message)
            }
        }
    }

    fn stop_loading(&mut self) {
        for load in &self.loads {
            load.stop();
        }
        self.loads.clear();
    }
}

/// What the user typed, as a value of `id`'s kind: bits for logic
/// signals (read in the signal's radix), names for enumerations and
/// booleans (nvc writes them in lower case), numbers for reals.
fn target_value(wave: &Waveform, id: SignalId, radix: Radix, text: &str) -> Result<Value, String> {
    match wave.signal(id).kind {
        SignalKind::Logic { width } => parse_logic_value(text, radix, width as usize).map(Value::Logic),
        SignalKind::Text => match text.trim() {
            "" => Err("type a value".to_string()),
            name => Ok(Value::Text(name.to_lowercase())),
        },
        SignalKind::Real => text.trim().parse().map(Value::Real).map_err(|_| format!("'{}' is not a number", text.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/waves").join(name)
    }

    /// Polls `state` whenever woken until `done` says so.
    fn wait_until(state: &mut WaveState, woken: &mpsc::Receiver<()>, done: impl Fn(&WaveState) -> bool) {
        while !done(state) {
            woken.recv_timeout(Duration::from_secs(30)).expect("took too long");
            state.poll();
        }
    }

    fn waker() -> (impl Fn() + Send + Clone + 'static, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        (
            move || {
                let _ = sender.send(());
            },
            receiver,
        )
    }

    #[test]
    fn opens_adds_and_loads_signals() {
        let mut state = WaveState::default();
        let (wake, woken) = waker();
        state.open(&fixture("types_tb.fst"), wake.clone());
        assert!(state.is_opening());
        wait_until(&mut state, &woken, |s| s.wave.is_some());
        assert!(state.needs_fit);
        assert_eq!(state.path(), Some(fixture("types_tb.fst")));

        let wave = state.wave.as_ref().unwrap();
        let sl = wave.find_signal("types_tb.sl").unwrap();
        let snum = wave.find_signal("types_tb.snum").unwrap();
        let state_sig = wave.find_signal("types_tb.state").unwrap();
        state.add(&[sl, snum, state_sig, sl], wake);
        assert_eq!(state.signals.len(), 3, "sl only once");
        assert_eq!(state.signals[0].label, "sl");
        assert!(state.signals[0].is_bit);
        assert_eq!(state.signals[1].radix, Radix::Signed, "signed shows as signed decimal");
        assert!(!state.signals[2].is_bit);

        wait_until(&mut state, &woken, |s| s.loading_progress().is_none());
        let wave = state.wave.as_ref().unwrap();
        assert!(state.signals.iter().all(|shown| wave.is_loaded(shown.id)));

        state.remove(0);
        assert_eq!(state.signals.len(), 2);
        assert_eq!(state.signals[0].label, "snum");
    }

    #[test]
    fn a_bad_file_gives_an_error() {
        let mut state = WaveState::default();
        let (wake, woken) = waker();
        state.open(&fixture("no_such_file.fst"), wake);
        wait_until(&mut state, &woken, |s| !s.is_opening());
        assert!(state.wave.is_none());
        assert!(state.error.as_deref().unwrap().contains("file not found"));
    }

    /// types_tb.fst open with `paths` added and loaded, the first selected.
    fn types_with(paths: &[&str]) -> WaveState {
        let mut state = WaveState::default();
        let (wake, woken) = waker();
        state.open(&fixture("types_tb.fst"), wake.clone());
        wait_until(&mut state, &woken, |s| s.wave.is_some());
        let wave = state.wave.as_ref().unwrap();
        let ids: Vec<SignalId> = paths.iter().map(|p| wave.find_signal(p).unwrap()).collect();
        state.add(&ids, wake);
        wait_until(&mut state, &woken, |s| s.loading_progress().is_none());
        state.selected = Some(0);
        state
    }

    fn ns(value: u64) -> SimTime {
        SimTime::from_femtoseconds(value * 1_000_000)
    }

    #[test]
    fn stepping_through_edges() {
        // sl changes every 10 ns: U X Z W - L H at 0, 10, ..., 60 ns.
        let mut state = types_with(&["types_tb.sl"]);
        state.cursors.primary = Some(ns(15));
        assert_eq!(state.step_edge(true), Ok(ns(20)));
        assert_eq!(state.step_edge(true), Ok(ns(30)));
        assert_eq!(state.reveal_request, Some(ns(30)), "the view follows");
        assert_eq!(state.step_edge(false), Ok(ns(20)));
        state.cursors.primary = Some(ns(60));
        assert!(state.step_edge(true).unwrap_err().contains("no edge after 60 ns"));
        assert_eq!(state.cursors.primary, Some(ns(60)), "the cursor stays put");
    }

    #[test]
    fn finding_values_of_each_kind() {
        let mut state = types_with(&["types_tb.sl", "types_tb.bus8", "types_tb.state", "types_tb.r"]);
        let find = |state: &mut WaveState, row: usize, text: &str| {
            state.selected = Some(row);
            state.cursors.primary = None;
            state.find_text = text.to_string();
            state.find_value(true)
        };
        assert_eq!(find(&mut state, 0, "W"), Ok(ns(30)));
        assert_eq!(find(&mut state, 1, "A5"), Ok(ns(20)), "hex, the bus's radix");
        assert_eq!(find(&mut state, 1, "0b01XZWLH-"), Ok(ns(10)));
        assert_eq!(find(&mut state, 2, "DATA"), Ok(ns(30)), "enum names in any case");
        assert_eq!(find(&mut state, 3, "3.25"), Ok(ns(50)));
        assert!(find(&mut state, 1, "77").unwrap_err().contains("no '77'"));
        assert!(find(&mut state, 1, "0x1FF").unwrap_err().contains("too big"));

        // Backwards from a cursor.
        state.selected = Some(0);
        state.find_text = "X".to_string();
        state.cursors.primary = Some(ns(55));
        assert_eq!(state.find_value(false), Ok(ns(10)));
    }

    #[test]
    fn navigation_needs_a_selected_signal() {
        let mut state = types_with(&["types_tb.sl"]);
        state.selected = None;
        assert!(state.step_edge(true).unwrap_err().contains("select a signal"));
    }

    #[test]
    fn filtering_the_signal_list() {
        let mut state = types_with(&[]);
        let names = |state: &mut WaveState| -> Vec<String> {
            let ids = state.filtered_signals().to_vec();
            let wave = state.wave.as_ref().unwrap();
            ids.iter().map(|&id| wave.signal(id).path).collect()
        };
        state.tree_filter = "COUNT".to_string();
        assert_eq!(names(&mut state), ["types_tb.count", "types_tb.rec.count"]);
        state.tree_filter = "inner".to_string();
        assert_eq!(names(&mut state), ["types_tb.nested.inner"]);
        state.tree_filter = "nothing like this".to_string();
        assert!(names(&mut state).is_empty());
    }

    #[test]
    fn opening_another_file_clears_the_view() {
        let mut state = WaveState::default();
        let (wake, woken) = waker();
        state.open(&fixture("types_tb.fst"), wake.clone());
        wait_until(&mut state, &woken, |s| s.wave.is_some());
        let sl = state.wave.as_ref().unwrap().find_signal("types_tb.sl").unwrap();
        state.add(&[sl], wake.clone());

        state.open(&fixture("resolution_tb.fst"), wake);
        assert!(state.signals.is_empty());
        assert!(state.wave.is_none(), "the old waveform is dropped straight away");
        wait_until(&mut state, &woken, |s| s.wave.is_some());
        assert_eq!(state.path(), Some(fixture("resolution_tb.fst")));
    }
}
