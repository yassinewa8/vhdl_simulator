//! Saving and restoring the wave view's setup ("layout"): which signals
//! are shown and how (order, groups, dividers, names, radix, colour), the
//! zoom, the cursors and the markers.
//!
//! Layouts are small TOML files meant to be read and kept in version
//! control, like a simulator's wave setup script. Signals are stored by
//! their path, so a layout still works after the design is changed and
//! re-simulated; signals that no longer exist are skipped and reported.
//!
//! No egui here, so saving and restoring can be tested.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use studio_core::{Radix, SimTime};
use wave::{SignalId, SignalKind, Waveform};

use crate::cursors::{Cursors, Marker};
use crate::timeline::Timeline;
use crate::wave_list::{Group, Item, SignalList};
use crate::wave_state::{DisplayedSignal, WaveState};
use crate::wave_style::WaveColour;

/// Bumped if the format ever changes in a way old files can't follow.
const FORMAT_VERSION: u32 = 1;

/// Everything in a layout file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub version: u32,
    /// The visible time range. Missing: zoom to fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<View>,
    #[serde(default)]
    pub cursors: CursorsLayout,
    /// The rows of the view, top to bottom.
    #[serde(default, rename = "row", skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<RowLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct View {
    /// Time at the left edge, in femtoseconds.
    pub start_fs: f64,
    /// Zoom: femtoseconds per pixel.
    pub fs_per_pixel: f64,
}

/// Times are written as text (`"15 ns"`), which is readable and has no
/// size limit (TOML integers stop at about 2.5 hours of femtoseconds).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CursorsLayout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    #[serde(default, rename = "marker", skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<MarkerLayout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerLayout {
    pub name: String,
    pub time: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RowLayout {
    Signal(SignalLayout),
    Divider {
        #[serde(default)]
        label: String,
    },
    Group {
        name: String,
        #[serde(default)]
        collapsed: bool,
        #[serde(default)]
        signals: Vec<SignalLayout>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalLayout {
    /// Full dotted path, e.g. `top_tb.dut.count`.
    pub path: String,
    /// The alias, if renamed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
}

impl Layout {
    /// The current setup of `waves`.
    pub fn capture(waves: &WaveState) -> Layout {
        let rows = waves
            .list
            .items()
            .iter()
            .map(|item| match item {
                Item::Signal(signal) => RowLayout::Signal(signal_layout(signal)),
                Item::Divider(label) => RowLayout::Divider { label: label.clone() },
                Item::Group(group) => RowLayout::Group {
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    signals: group.signals.iter().map(signal_layout).collect(),
                },
            })
            .collect();
        let time_text = |time: SimTime| time.to_string();
        Layout {
            version: FORMAT_VERSION,
            view: Some(View {
                start_fs: waves.timeline.start(),
                fs_per_pixel: waves.timeline.scale(),
            }),
            cursors: CursorsLayout {
                primary: waves.cursors.primary.map(time_text),
                secondary: waves.cursors.secondary.map(time_text),
                markers: waves
                    .cursors
                    .markers
                    .iter()
                    .map(|m| MarkerLayout {
                        name: m.name.clone(),
                        time: time_text(m.time),
                    })
                    .collect(),
            },
            rows,
        }
    }

    /// Applies this layout to `waves`, whose waveform must be open.
    /// Returns the paths of signals that aren't in the waveform (skipped).
    /// The caller still has to load the shown signals' data.
    pub fn restore(&self, waves: &mut WaveState) -> Vec<String> {
        let Some(wave) = &waves.wave else { return Vec::new() };
        let ids = ids_by_path(wave);
        let mut missing = Vec::new();
        let mut make = |layout: &SignalLayout| -> Option<DisplayedSignal> {
            match ids.get(&layout.path.to_lowercase()) {
                Some(&id) => Some(displayed_signal(wave, id, layout)),
                None => {
                    missing.push(layout.path.clone());
                    None
                }
            }
        };
        let items: Vec<Item> = self
            .rows
            .iter()
            .filter_map(|row| match row {
                RowLayout::Signal(layout) => make(layout).map(Item::Signal),
                RowLayout::Divider { label } => Some(Item::Divider(label.clone())),
                RowLayout::Group { name, collapsed, signals } => Some(Item::Group(Group {
                    name: name.clone(),
                    collapsed: *collapsed,
                    signals: signals.iter().filter_map(&mut make).collect(),
                })),
            })
            .collect();
        waves.list = SignalList::from_items(items);

        let parse = |text: &Option<String>| text.as_deref().and_then(SimTime::parse);
        let mut cursors = Cursors::new(parse(&self.cursors.primary), parse(&self.cursors.secondary));
        for marker in &self.cursors.markers {
            if let Some(time) = SimTime::parse(&marker.time) {
                let index = cursors.add_marker(time);
                cursors.markers[index] = Marker {
                    name: marker.name.clone(),
                    time,
                };
            }
        }
        waves.cursors = cursors;
        match self.view {
            Some(view) => {
                waves.timeline = Timeline::from_parts(view.start_fs, view.fs_per_pixel);
                waves.needs_fit = false;
            }
            None => waves.needs_fit = true,
        }
        waves.select_only(None);
        missing
    }

    /// Reads a layout file; `None` if there isn't one or it can't be read
    /// (a damaged layout is ignored rather than stopping the waveform
    /// from opening).
    pub fn load(path: &Path) -> Option<Layout> {
        let text = std::fs::read_to_string(path).ok()?;
        let layout: Layout = toml::from_str(&text).ok()?;
        (layout.version <= FORMAT_VERSION).then_some(layout)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = toml::to_string_pretty(self).expect("layouts always convert to TOML");
        std::fs::write(path, format!("# Wave view layout (NVC Studio). Safe to edit or delete.\n{text}"))
    }
}

fn signal_layout(signal: &DisplayedSignal) -> SignalLayout {
    SignalLayout {
        path: signal.path.clone(),
        name: (signal.label != signal.original_name()).then(|| signal.label.clone()),
        radix: Some(radix_key(signal.radix).to_string()),
        colour: (signal.colour != WaveColour::Default).then(|| colour_key(signal.colour).to_string()),
    }
}

/// A shown signal built from its saved settings.
fn displayed_signal(wave: &Waveform, id: SignalId, layout: &SignalLayout) -> DisplayedSignal {
    let info = wave.signal(id);
    let (width, is_bit) = match info.kind {
        SignalKind::Logic { width } => (width as usize, width == 1),
        SignalKind::Text | SignalKind::Real => (0, false),
    };
    DisplayedSignal {
        id,
        label: layout.name.clone().unwrap_or_else(|| info.name.clone()),
        radix: layout
            .radix
            .as_deref()
            .and_then(radix_from_key)
            .unwrap_or_else(|| studio_core::default_radix(info.vhdl_type.as_deref(), width)),
        path: info.path,
        is_bit,
        colour: layout.colour.as_deref().and_then(colour_from_key).unwrap_or_default(),
    }
}

/// Every signal by lower-case path, built once per restore (looking each
/// up separately would read the whole tree once per signal).
fn ids_by_path(wave: &Waveform) -> HashMap<String, SignalId> {
    wave.all_signals()
        .into_iter()
        .map(|id| (wave.signal(id).path.to_lowercase(), id))
        .collect()
}

const RADIX_KEYS: [(Radix, &str); 6] = [
    (Radix::Binary, "binary"),
    (Radix::Hex, "hex"),
    (Radix::Octal, "octal"),
    (Radix::Unsigned, "unsigned"),
    (Radix::Signed, "signed"),
    (Radix::Ascii, "ascii"),
];

fn radix_key(radix: Radix) -> &'static str {
    RADIX_KEYS.iter().find(|(r, _)| *r == radix).map_or("hex", |(_, key)| key)
}

fn radix_from_key(key: &str) -> Option<Radix> {
    RADIX_KEYS.iter().find(|(_, k)| k.eq_ignore_ascii_case(key)).map(|(radix, _)| *radix)
}

fn colour_key(colour: WaveColour) -> &'static str {
    match colour {
        WaveColour::Default => "default",
        WaveColour::Yellow => "yellow",
        WaveColour::Cyan => "cyan",
        WaveColour::Orange => "orange",
        WaveColour::Pink => "pink",
        WaveColour::White => "white",
        WaveColour::Blue => "blue",
    }
}

fn colour_from_key(key: &str) -> Option<WaveColour> {
    WaveColour::ALL.into_iter().find(|&c| colour_key(c).eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for radix in Radix::ALL {
            assert_eq!(radix_from_key(radix_key(radix)), Some(radix));
        }
        for colour in WaveColour::ALL {
            assert_eq!(colour_from_key(colour_key(colour)), Some(colour));
        }
        assert_eq!(radix_from_key("HEX"), Some(Radix::Hex));
        assert_eq!(radix_from_key("nonsense"), None);
    }

    #[test]
    fn the_file_format_is_readable() {
        let layout = Layout {
            version: 1,
            view: Some(View {
                start_fs: 0.0,
                fs_per_pixel: 1000.0,
            }),
            cursors: CursorsLayout {
                primary: Some("15 ns".to_string()),
                secondary: None,
                markers: vec![MarkerLayout {
                    name: "reset done".to_string(),
                    time: "100 ns".to_string(),
                }],
            },
            rows: vec![
                RowLayout::Signal(SignalLayout {
                    path: "tb.clk".to_string(),
                    name: None,
                    radix: Some("binary".to_string()),
                    colour: Some("yellow".to_string()),
                }),
                RowLayout::Divider { label: "bus".to_string() },
                RowLayout::Group {
                    name: "uart".to_string(),
                    collapsed: true,
                    signals: vec![SignalLayout {
                        path: "tb.tx".to_string(),
                        name: Some("serial out".to_string()),
                        radix: None,
                        colour: None,
                    }],
                },
            ],
        };
        let text = toml::to_string_pretty(&layout).unwrap();
        assert!(text.contains("kind = \"signal\""), "{text}");
        assert!(text.contains("path = \"tb.clk\""), "{text}");
        assert!(text.contains("primary = \"15 ns\""), "{text}");
        let back: Layout = toml::from_str(&text).unwrap();
        assert_eq!(back, layout);
    }

    #[test]
    fn damaged_or_future_files_are_ignored() {
        let dir = std::env::temp_dir().join("nvc-studio-tests").join("layout_damaged");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.toml");
        std::fs::write(&path, "this is [not toml").unwrap();
        assert_eq!(Layout::load(&path), None);
        std::fs::write(&path, "version = 99\n").unwrap();
        assert_eq!(Layout::load(&path), None);
        assert_eq!(Layout::load(&dir.join("missing.toml")), None);
    }
}
