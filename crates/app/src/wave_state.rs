//! What the Waves tab shows: the open waveform, the signals added to the
//! view, and the visible time range.
//!
//! No egui here: opening and loading happen on background threads (from
//! the wave crate) and this only keeps track of them, so it can be tested.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use studio_core::{Radix, SimTime, Value, default_radix, parse_logic_value};
use wave::{BackgroundLoad, BackgroundOpen, SignalId, SignalKind, Waveform, open_in_background};

use crate::cursors::{CursorLine, Cursors};
use crate::layout::Layout;
use crate::timeline::Timeline;
use crate::wave_list::{Drop, RowRef, SignalList};
use crate::wave_style::WaveColour;

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
    pub colour: WaveColour,
}

impl DisplayedSignal {
    /// The last part of the path: the name before any renaming.
    pub fn original_name(&self) -> &str {
        self.path.rsplit('.').next().unwrap_or(&self.path)
    }
}

#[derive(Default)]
pub struct WaveState {
    /// A file being opened in the background.
    opening: Option<BackgroundOpen>,
    pub wave: Option<Waveform>,
    /// Why the last open failed, to show in the Waves tab.
    pub error: Option<String>,
    /// The rows of the view: signals, dividers and groups.
    pub list: SignalList,
    /// Signals being read from the file in the background.
    loads: Vec<BackgroundLoad>,
    pub timeline: Timeline,
    /// Set when a new waveform arrives: the view should zoom to fit once
    /// it knows how wide it is.
    pub needs_fit: bool,
    /// The current row: where a Shift+click range starts, and the signal
    /// navigation (next edge, find value) works on.
    pub selected: Option<RowRef>,
    /// Every selected row (Ctrl+click adds, Shift+click selects a range).
    pub selection: Vec<RowRef>,
    /// How far the signal list is scrolled down, in pixels.
    pub row_scroll: f32,
    /// Zoom asked for by a toolbar button, applied around the centre on
    /// the next frame (the button doesn't know the wave area's size).
    pub zoom_request: Option<f64>,
    /// The time under the mouse, shown in the toolbar, already formatted
    /// to the precision of the current zoom.
    pub hover_time: Option<String>,
    /// While hovering near an edge: where a click would put the cursor,
    /// and the on-screen row (counting from the top) of that signal.
    pub hover_snap: Option<(SimTime, usize)>,
    pub cursors: Cursors,
    /// A time to centre the view on next frame (e.g. "go to marker").
    pub centre_request: Option<SimTime>,
    /// A time to bring into view next frame, if it's off-screen (e.g.
    /// after jumping to the next edge).
    pub reveal_request: Option<SimTime>,
    /// The cursor or marker being dragged with the mouse.
    pub dragging: Option<CursorLine>,
    /// The row being dragged in the names column, to reorder it.
    pub row_drag: Option<RowRef>,
    /// Where that row would land if let go now (for drawing a guide).
    pub row_drop: Option<Drop>,
    /// What the "find value" box holds.
    pub find_text: String,
    /// Why the last edge step or find didn't move the cursor.
    pub find_message: Option<String>,
    /// What the search box above the signal tree holds.
    pub tree_filter: String,
    /// The signals matching `tree_filter`, worked out once per filter text.
    filter_cache: Option<(String, Vec<SignalId>)>,
    /// Signals in the restored layout that this waveform doesn't have.
    pub missing_signals: Vec<String>,
    /// Asks the window to redraw; kept for loads started later.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Where this waveform's layout is saved (`None`: not saved).
    layout_file: Option<PathBuf>,
    /// A layout to apply as soon as the waveform has opened.
    pending_layout: Option<Layout>,
    /// The layout as last saved or restored, to notice changes.
    saved_layout: Option<Layout>,
    /// When the layout first differed from the saved one.
    unsaved_since: Option<Instant>,
    /// The file's modification time when it was opened.
    file_stamp: Option<SystemTime>,
    /// A newer modification time, and when it was first seen.
    new_stamp: Option<(SystemTime, Instant)>,
    /// When the file was last checked for changes.
    last_file_check: Option<Instant>,
}

/// The layout is saved this long after the last change to it.
const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);
/// A changed waveform file is reloaded once it has been unchanged this
/// long (so a simulator still writing it isn't caught half-way).
const RELOAD_SETTLE: Duration = Duration::from_secs(1);
/// How often to look at the waveform file's modification time.
const FILE_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// The search box shows at most this many matches.
pub const MAX_FILTER_RESULTS: usize = 500;

impl WaveState {
    /// Starts opening `path` with no saved layout, replacing whatever was
    /// shown. (The app always opens with a layout file; tests often don't.)
    #[cfg(test)]
    pub fn open(&mut self, path: &Path, wake: impl Fn() + Send + Sync + 'static) {
        self.open_with_layout(path, None, None, wake);
    }

    /// Starts opening `path`, replacing whatever was shown. Once open, the
    /// view is set up from `keep` if given (a reload), otherwise from the
    /// layout saved in `layout_file`, which is also where changes are
    /// saved from now on.
    pub fn open_with_layout(
        &mut self,
        path: &Path,
        layout_file: Option<PathBuf>,
        keep: Option<Layout>,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        self.save_layout_now();
        self.stop_loading();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let open_wake = wake.clone();
        self.wake = Some(wake);
        self.pending_layout = keep.or_else(|| layout_file.as_deref().and_then(Layout::load));
        self.saved_layout = None;
        self.unsaved_since = None;
        self.layout_file = layout_file;
        self.missing_signals.clear();
        self.file_stamp = modified_time(path);
        self.new_stamp = None;
        self.opening = Some(open_in_background(path, move || open_wake()));
        self.wave = None;
        self.error = None;
        self.list = SignalList::default();
        self.selected = None;
        self.selection.clear();
        self.row_scroll = 0.0;
        self.cursors = Cursors::default();
        self.dragging = None;
        self.row_drag = None;
        self.row_drop = None;
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
                    if let Some(layout) = self.pending_layout.take() {
                        self.missing_signals = layout.restore(self);
                        self.load_shown();
                        self.saved_layout = Some(Layout::capture(self));
                    }
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
            if self.list.contains(id) {
                continue;
            }
            let info = wave.signal(id);
            let (width, is_bit) = match info.kind {
                SignalKind::Logic { width } => (width as usize, width == 1),
                SignalKind::Text | SignalKind::Real => (0, false),
            };
            self.list.push_signal(DisplayedSignal {
                id,
                label: info.name.clone(),
                path: info.path,
                radix: default_radix(info.vhdl_type.as_deref(), width),
                is_bit,
                colour: WaveColour::Default,
            });
            added.push(id);
        }
        if !added.is_empty() {
            self.loads.push(wave.load_in_background(&added, wake));
        }
    }

    /// Removes a row (a group header removes the whole group).
    pub fn remove(&mut self, row: RowRef) {
        self.list.remove(row);
        self.select_only(None);
    }

    /// A plain click: just this row (or nothing).
    pub fn select_only(&mut self, row: Option<RowRef>) {
        self.selected = row;
        self.selection = row.into_iter().collect();
    }

    /// Ctrl+click: adds the row to the selection, or takes it out.
    pub fn toggle_selected(&mut self, row: RowRef) {
        if let Some(index) = self.selection.iter().position(|&r| r == row) {
            self.selection.remove(index);
            if self.selected == Some(row) {
                self.selected = self.selection.last().copied();
            }
        } else {
            self.selection.push(row);
            self.selected = Some(row);
        }
    }

    /// Shift+click: every row on screen from the current row to `row`.
    pub fn select_range_to(&mut self, row: RowRef) {
        let rows = self.list.rows();
        let Some(anchor) = self.selected.and_then(|a| rows.iter().position(|&r| r == a)) else {
            return self.select_only(Some(row));
        };
        let Some(end) = rows.iter().position(|&r| r == row) else { return };
        let (from, to) = (anchor.min(end), anchor.max(end));
        self.selection = rows[from..=to].to_vec();
        // The anchor stays, so the range can be changed with another Shift+click.
    }

    pub fn is_selected(&self, row: RowRef) -> bool {
        self.selection.contains(&row)
    }

    /// Puts the selected signals into a new group (Ctrl+G).
    pub fn group_selection(&mut self) {
        let header = self.list.group_rows(&self.selection);
        self.select_only(header);
    }

    /// Removes every selected row (Delete).
    pub fn remove_selection(&mut self) {
        self.list.remove_rows(&self.selection);
        self.select_only(None);
    }

    /// Moves the selected signals (or just `row`, if it isn't selected)
    /// to `drop`, keeping the moved rows selected.
    pub fn move_dragged(&mut self, row: RowRef, drop: Drop) {
        if self.is_selected(row) && self.selection.len() > 1 {
            let moved = self.list.move_rows(&self.selection, drop);
            self.selected = moved.first().copied();
            self.selection = moved;
        } else {
            let moved = self.list.apply_drop(row, drop);
            self.select_only(moved);
        }
    }

    /// Starts reading the data of every shown signal not loaded yet.
    fn load_shown(&mut self) {
        let (Some(wave), Some(wake)) = (&self.wave, &self.wake) else { return };
        let ids: Vec<SignalId> = self.list.signals().map(|s| s.id).filter(|&id| !wave.is_loaded(id)).collect();
        if !ids.is_empty() {
            let wake = wake.clone();
            self.loads.push(wave.load_in_background(&ids, move || wake()));
        }
    }

    /// Opens the same file again, keeping the current setup (signals,
    /// zoom, cursors...). Used after a re-run.
    pub fn reload(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        let Some(path) = self.path() else { return };
        let keep = self.wave.is_some().then(|| Layout::capture(self));
        let layout_file = self.layout_file.clone();
        self.open_with_layout(&path, layout_file, keep, wake);
    }

    /// Saves the layout if it changed and has stayed unchanged for a
    /// moment. Call once per frame.
    pub fn autosave(&mut self, now: Instant) {
        if !self.layout_worth_saving() {
            return;
        }
        let current = Layout::capture(self);
        if self.saved_layout.as_ref() == Some(&current) {
            self.unsaved_since = None;
            return;
        }
        match self.unsaved_since {
            None => self.unsaved_since = Some(now),
            Some(since) if now.duration_since(since) >= AUTOSAVE_DELAY => self.write_layout(current),
            Some(_) => {}
        }
    }

    /// Saves the layout straight away if it changed (e.g. when closing).
    pub fn save_layout_now(&mut self) {
        if self.layout_worth_saving() {
            let current = Layout::capture(self);
            if self.saved_layout.as_ref() != Some(&current) {
                self.write_layout(current);
            }
        }
    }

    /// Reloads the waveform if its file changed on disk (e.g. re-simulated
    /// from the command line) and has settled. Call once per frame.
    pub fn check_file(&mut self, now: Instant, wake: impl Fn() + Send + Sync + 'static) {
        if self.is_opening() || self.last_file_check.is_some_and(|last| now.duration_since(last) < FILE_CHECK_INTERVAL) {
            return;
        }
        self.last_file_check = Some(now);
        let Some(path) = self.path() else { return };
        let Some(stamp) = modified_time(&path) else { return };
        if Some(stamp) == self.file_stamp {
            self.new_stamp = None;
            return;
        }
        match self.new_stamp {
            Some((seen, since)) if seen == stamp && now.duration_since(since) >= RELOAD_SETTLE => self.reload(wake),
            Some((seen, _)) if seen == stamp => {}
            _ => self.new_stamp = Some((stamp, now)),
        }
    }

    /// Only save when there's somewhere to save to, and something worth
    /// saving: a setup with signals, or one that was saved before (so
    /// clearing it is saved too). Opening a file and looking doesn't
    /// leave layout files behind.
    fn layout_worth_saving(&self) -> bool {
        self.wave.is_some()
            && self.pending_layout.is_none()
            && self.layout_file.as_ref().is_some_and(|file| !self.list.is_empty() || file.exists())
    }

    fn write_layout(&mut self, layout: Layout) {
        if let Some(file) = &self.layout_file {
            // A failed save is retried at the next change; it shouldn't
            // interrupt looking at waves.
            let _ = layout.save(file);
        }
        self.saved_layout = Some(layout);
        self.unsaved_since = None;
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
        let radix = self.selected.and_then(|row| self.list.signal(row)).map_or(Radix::Hex, |s| s.radix);
        let target = target_value(wave, id, radix, &self.find_text)?;
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
        let signal = self
            .selected
            .and_then(|row| self.list.signal(row))
            .ok_or("select a signal first (click its row)")?;
        Ok((wave, signal.id))
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

/// When `path` was last changed, if it can be read.
fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
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
    use crate::wave_style::WaveColour;
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

    fn waker() -> (impl Fn() + Send + Sync + Clone + 'static, mpsc::Receiver<()>) {
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
        let shown: Vec<&DisplayedSignal> = state.list.signals().collect();
        assert_eq!(shown.len(), 3, "sl only once");
        assert_eq!(shown[0].label, "sl");
        assert!(shown[0].is_bit);
        assert_eq!(shown[1].radix, Radix::Signed, "signed shows as signed decimal");
        assert!(!shown[2].is_bit);
        assert_eq!(shown[2].original_name(), "state");

        wait_until(&mut state, &woken, |s| s.loading_progress().is_none());
        let wave = state.wave.as_ref().unwrap();
        assert!(state.list.signals().all(|shown| wave.is_loaded(shown.id)));

        state.remove(RowRef::top(0));
        assert_eq!(state.list.signals().count(), 2);
        assert_eq!(state.list.signals().next().unwrap().label, "snum");
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
        state.selected = Some(RowRef::top(0));
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
            state.selected = Some(RowRef::top(row));
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
        state.selected = Some(RowRef::top(0));
        state.find_text = "X".to_string();
        state.cursors.primary = Some(ns(55));
        assert_eq!(state.find_value(false), Ok(ns(10)));
    }

    #[test]
    fn navigation_needs_a_selected_signal() {
        let mut state = types_with(&["types_tb.sl"]);
        state.selected = None;
        assert!(state.step_edge(true).unwrap_err().contains("select a signal"));
        // A group header or divider isn't a signal either.
        let divider = state.list.add_divider_after(None);
        state.selected = Some(divider);
        assert!(state.step_edge(true).unwrap_err().contains("select a signal"));
    }

    #[test]
    fn selecting_several_rows() {
        let mut state = types_with(&["types_tb.sl", "types_tb.bus8", "types_tb.snum", "types_tb.state"]);
        let row = RowRef::top;
        state.select_only(Some(row(1)));
        state.select_range_to(row(3));
        assert_eq!(state.selection, [row(1), row(2), row(3)]);
        assert_eq!(state.selected, Some(row(1)), "the range starts from the current row");
        state.select_range_to(row(0));
        assert_eq!(state.selection, [row(0), row(1)]);

        state.select_only(Some(row(0)));
        state.toggle_selected(row(2));
        assert!(state.is_selected(row(0)) && state.is_selected(row(2)) && !state.is_selected(row(1)));
        assert_eq!(state.selected, Some(row(2)));
        state.toggle_selected(row(2));
        assert_eq!(state.selection, [row(0)]);
        assert_eq!(state.selected, Some(row(0)));

        // Group sl and snum, then move both signals out together.
        state.toggle_selected(row(2));
        state.group_selection();
        let labels = |state: &WaveState| -> Vec<String> { state.list.signals().map(|s| s.label.clone()).collect() };
        assert_eq!(labels(&state), ["sl", "snum", "bus8", "state"]);
        assert_eq!(state.selected, Some(row(0)), "the new group's header is selected");

        state.select_only(Some(RowRef { item: 0, child: Some(0) }));
        state.toggle_selected(RowRef { item: 0, child: Some(1) });
        state.move_dragged(RowRef { item: 0, child: Some(0) }, Drop::Before(None));
        assert_eq!(labels(&state), ["bus8", "state", "sl", "snum"]);
        assert_eq!(state.selection.len(), 2, "the moved rows stay selected");

        state.remove_selection();
        assert_eq!(labels(&state), ["bus8", "state"]);
        assert!(state.selection.is_empty());
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

    /// A scratch copy of a fixture, so a test can change or touch it.
    fn scratch_copy(name: &str, test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("nvc-studio-tests").join(test);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join(name);
        std::fs::copy(fixture(name), &copy).unwrap();
        copy
    }

    /// Opens `file` with `layout_file` and waits until it and its shown
    /// signals are loaded. Returns the wake-up channel it was opened with.
    fn open_and_wait(state: &mut WaveState, file: &Path, layout_file: &Path) -> mpsc::Receiver<()> {
        let (wake, woken) = waker();
        state.open_with_layout(file, Some(layout_file.to_path_buf()), None, wake);
        wait_until(state, &woken, |s| s.wave.is_some() && s.loading_progress().is_none());
        woken
    }

    #[test]
    fn layouts_are_saved_and_restored() {
        let file = scratch_copy("types_tb.fst", "layouts_are_saved_and_restored");
        let layout_file = file.with_extension("layout.toml");
        let mut state = WaveState::default();
        open_and_wait(&mut state, &file, &layout_file);
        let (wake, woken) = waker();
        let wave = state.wave.as_ref().unwrap();
        let ids: Vec<SignalId> = ["types_tb.sl", "types_tb.bus8", "types_tb.state"]
            .iter()
            .map(|p| wave.find_signal(p).unwrap())
            .collect();
        state.add(&ids, wake);
        wait_until(&mut state, &woken, |s| s.loading_progress().is_none());

        // Change one of everything.
        let bus = RowRef::top(1);
        state.list.signal_mut(bus).unwrap().label = "data".to_string();
        state.list.signal_mut(bus).unwrap().radix = Radix::Binary;
        state.list.signal_mut(bus).unwrap().colour = WaveColour::Cyan;
        state.list.add_divider_after(Some(RowRef::top(0)));
        state.list.group_rows(&[RowRef::top(3)]);
        state.cursors.primary = Some(ns(25));
        state.cursors.secondary = Some(ns(35));
        state.cursors.add_marker(ns(40));
        state.timeline = Timeline::from_parts(5e6, 1e5);
        state.save_layout_now();
        assert!(layout_file.is_file());
        let saved = Layout::capture(&state);

        // A fresh view of the same file comes back the same.
        let mut again = WaveState::default();
        open_and_wait(&mut again, &file, &layout_file);
        assert_eq!(Layout::capture(&again), saved);
        assert!(again.missing_signals.is_empty());
        let wave = again.wave.as_ref().unwrap();
        assert!(again.list.signals().all(|s| wave.is_loaded(s.id)), "shown signals are loaded");
        assert!(!again.needs_fit, "the saved zoom is used, not fit");
    }

    #[test]
    fn signals_missing_from_the_waveform_are_skipped_and_reported() {
        let file = scratch_copy("types_tb.fst", "signals_missing_are_reported");
        let layout_file = file.with_extension("layout.toml");
        std::fs::write(
            &layout_file,
            "version = 1\n[[row]]\nkind = \"signal\"\npath = \"types_tb.gone\"\n[[row]]\nkind = \"signal\"\npath = \"TYPES_TB.SL\"\n",
        )
        .unwrap();
        let mut state = WaveState::default();
        open_and_wait(&mut state, &file, &layout_file);
        assert_eq!(state.missing_signals, ["types_tb.gone"]);
        assert_eq!(state.list.signals().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["sl"], "paths ignore case");
        assert!(state.needs_fit, "no saved zoom: fit");
    }

    #[test]
    fn reload_keeps_the_setup() {
        let file = scratch_copy("types_tb.fst", "reload_keeps_the_setup");
        let mut state = WaveState::default();
        open_and_wait(&mut state, &file, &file.with_extension("layout.toml"));
        let sl = state.wave.as_ref().unwrap().find_signal("types_tb.sl").unwrap();
        state.add(&[sl], || {});
        state.cursors.primary = Some(ns(25));
        let before = Layout::capture(&state);

        let (wake, woken) = waker();
        state.reload(wake);
        assert!(state.is_opening());
        wait_until(&mut state, &woken, |s| s.wave.is_some() && s.loading_progress().is_none());
        assert_eq!(Layout::capture(&state), before);
        assert!(state.wave.as_ref().unwrap().is_loaded(sl));
    }

    #[test]
    fn autosave_waits_for_changes_to_stop() {
        let file = scratch_copy("types_tb.fst", "autosave_waits");
        let layout_file = file.with_extension("layout.toml");
        let mut state = WaveState::default();
        open_and_wait(&mut state, &file, &layout_file);
        let start = Instant::now();
        state.autosave(start);
        assert!(!layout_file.exists(), "nothing worth saving yet");

        let sl = state.wave.as_ref().unwrap().find_signal("types_tb.sl").unwrap();
        state.add(&[sl], || {});
        state.autosave(start);
        assert!(!layout_file.exists(), "not straight away");
        state.autosave(start + Duration::from_millis(500));
        assert!(!layout_file.exists());
        state.autosave(start + Duration::from_millis(1100));
        assert!(layout_file.exists(), "a second after the change");
    }

    #[test]
    fn a_changed_file_reloads_once_it_settles() {
        let file = scratch_copy("types_tb.fst", "changed_file_reloads");
        let mut state = WaveState::default();
        open_and_wait(&mut state, &file, &file.with_extension("layout.toml"));
        let start = Instant::now();
        state.check_file(start, || {});
        assert!(!state.is_opening(), "unchanged: nothing to do");

        // As if re-simulated: the file gets a newer modification time.
        let later = SystemTime::now() + Duration::from_secs(10);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
        state.check_file(start + Duration::from_secs(1), || {});
        assert!(!state.is_opening(), "just changed: wait for it to settle");
        state.check_file(start + Duration::from_millis(1600), || {});
        assert!(!state.is_opening(), "still settling");
        state.check_file(start + Duration::from_millis(2200), || {});
        assert!(state.is_opening(), "settled: reloaded");
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
        assert!(state.list.is_empty());
        assert!(state.wave.is_none(), "the old waveform is dropped straight away");
        wait_until(&mut state, &woken, |s| s.wave.is_some());
        assert_eq!(state.path(), Some(fixture("resolution_tb.fst")));
    }
}
