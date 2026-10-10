//! What the Waves tab shows: the open waveform, the signals added to the
//! view, and the visible time range.
//!
//! No egui here: opening and loading happen on background threads (from
//! the wave crate) and this only keeps track of them, so it can be tested.

use std::path::{Path, PathBuf};

use studio_core::{Radix, default_radix};
use wave::{BackgroundLoad, BackgroundOpen, SignalId, SignalKind, Waveform, open_in_background};

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
}

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

    fn stop_loading(&mut self) {
        for load in &self.loads {
            load.stop();
        }
        self.loads.clear();
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
