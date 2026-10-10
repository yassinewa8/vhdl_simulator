//! Tests against real waveforms written by nvc, in tests/fixtures/waves/
//! (regenerate with `bash scripts/capture-nvc-fixtures.sh`). Each check
//! runs on both the FST and the VCD version of the same simulation.

use std::path::PathBuf;
use std::time::Duration;

use studio_core::{CancelToken, Logic, Radix, SimTime, Value, format_value, parse_logic_vector};
use wave::{ScopeKind, SignalKind, Waveform, open_in_background};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/waves").join(name)
}

fn open(name: &str) -> Waveform {
    Waveform::open(&fixture(name)).unwrap_or_else(|error| panic!("{error}"))
}

fn ns(value: u64) -> SimTime {
    SimTime::from_femtoseconds(value * 1_000_000)
}

/// Opens a fixture and loads every signal in it.
fn open_and_load_all(name: &str) -> Waveform {
    let mut wave = open(name);
    let all = wave.all_signals();
    wave.load(&all, &CancelToken::new());
    wave
}

fn value(wave: &Waveform, path: &str, time: SimTime) -> Value {
    let id = wave.find_signal(path).unwrap_or_else(|| panic!("no signal {path}"));
    wave.value_at(id, time).unwrap_or_else(|| panic!("{path} has no value at {time}"))
}

fn logic(text: &str) -> Value {
    Value::Logic(parse_logic_vector(text).unwrap())
}

fn text(text: &str) -> Value {
    Value::Text(text.to_string())
}

/// The resolution table from IEEE 1164 (std_logic_1164 package body),
/// copied from the standard: row = one driver, column = the other, both in
/// the order U X 0 1 Z W L H -.
const IEEE_RESOLUTION_TABLE: [&str; 9] = [
    "UUUUUUUUU", // U
    "UXXXXXXXX", // X
    "UX0X0000X", // 0
    "UXX11111X", // 1
    "UX01ZWLHX", // Z
    "UX01WWWWX", // W
    "UX01LWLWX", // L
    "UX01HWWHX", // H
    "UXXXXXXXX", // -
];

#[test]
fn std_logic_resolution_matches_the_ieee_table() {
    let order = parse_logic_vector("UX01ZWLH-").unwrap();
    for file in ["resolution_tb.fst", "resolution_tb.vcd"] {
        let wave = open_and_load_all(file);
        for (a, row) in IEEE_RESOLUTION_TABLE.iter().enumerate() {
            let expected_row = parse_logic_vector(row).unwrap();
            for b in 0..9 {
                // Step n runs from n*10 ns; look half-way through it.
                let time = ns((a * 9 + b) as u64 * 10 + 5);
                let context = format!("{file}: {} with {} at {time}", order[a], order[b]);
                assert_eq!(value(&wave, "resolution_tb.driver_a", time), Value::Logic(vec![order[a]]), "{context}");
                assert_eq!(value(&wave, "resolution_tb.driver_b", time), Value::Logic(vec![order[b]]), "{context}");
                assert_eq!(
                    value(&wave, "resolution_tb.resolved", time),
                    Value::Logic(vec![expected_row[b]]),
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn tree_of_scopes_and_signals() {
    for file in ["types_tb.fst", "types_tb.vcd"] {
        let wave = open(file);
        let top = wave.top_scopes();
        assert_eq!(top.len(), 1, "{file}");
        let tb = wave.scope(top[0]);
        assert_eq!((tb.name.as_str(), tb.kind), ("types_tb", ScopeKind::Instance), "{file}");

        let names: Vec<String> = tb.signals.iter().map(|&id| wave.signal(id).name).collect();
        assert_eq!(names, ["sl", "bus8", "snum", "state", "count", "flag", "b", "r", "t"], "{file}");

        let children: Vec<(String, ScopeKind)> =
            tb.scopes.iter().map(|&id| wave.scope(id)).map(|s| (s.name, s.kind)).collect();
        assert_eq!(
            children,
            [("rec".to_string(), ScopeKind::Record), ("nested".to_string(), ScopeKind::Block)],
            "{file}"
        );
        let rec = wave.scope(tb.scopes[0]);
        assert_eq!(rec.path, "types_tb.rec");
        let fields: Vec<String> = rec.signals.iter().map(|&id| wave.signal(id).path).collect();
        assert_eq!(fields, ["types_tb.rec.valid", "types_tb.rec.count"], "{file}");
    }
}

#[test]
fn signal_types_and_kinds() {
    for file in ["types_tb.fst", "types_tb.vcd"] {
        let wave = open(file);
        let info = |path: &str| wave.signal(wave.find_signal(path).unwrap());
        let check = |path: &str, vhdl_type: &str, kind: SignalKind| {
            let signal = info(path);
            assert_eq!(signal.vhdl_type.as_deref(), Some(vhdl_type), "{file} {path}");
            assert_eq!(signal.kind, kind, "{file} {path}");
        };
        check("types_tb.sl", "STD_LOGIC", SignalKind::Logic { width: 1 });
        check("types_tb.bus8", "STD_LOGIC_VECTOR", SignalKind::Logic { width: 8 });
        check("types_tb.snum", "SIGNED", SignalKind::Logic { width: 8 });
        check("types_tb.count", "INTEGER", SignalKind::Logic { width: 32 });
        check("types_tb.state", "STATE_T", SignalKind::Text);
        check("types_tb.flag", "BOOLEAN", SignalKind::Text);
        check("types_tb.r", "REAL", SignalKind::Real);
        check("types_tb.t", "TIME", SignalKind::Text);
        // Names are found regardless of case, like VHDL.
        assert!(wave.find_signal("TYPES_TB.REC.VALID").is_some());
        assert!(wave.find_signal("types_tb.no_such").is_none());
    }
}

#[test]
fn values_of_every_type_over_time() {
    for file in ["types_tb.fst", "types_tb.vcd"] {
        let wave = open_and_load_all(file);
        let at = |path: &str, time: u64| value(&wave, &format!("types_tb.{path}"), ns(time));

        // std_logic through its values: U at start, then X Z W - L H.
        let sl: Vec<Value> = [5, 15, 25, 35, 45, 55, 65].iter().map(|&t| at("sl", t)).collect();
        assert_eq!(sl, ["U", "X", "Z", "W", "-", "L", "H"].map(logic), "{file}");

        assert_eq!(at("bus8", 5), logic("UUUUUUUU"), "{file}");
        assert_eq!(at("bus8", 15), logic("01XZWLH-"), "{file}");
        assert_eq!(format_value(&at("bus8", 25), Radix::Hex), "A5", "{file}");
        assert_eq!(format_value(&at("snum", 25), Radix::Signed), "-3", "{file}");
        assert_eq!(format_value(&at("count", 35), Radix::Signed), "42", "{file}");
        assert_eq!(format_value(&at("count", 55), Radix::Signed), "1000", "{file}");

        // Enumerations show their names (nvc writes them in lower case).
        let states: Vec<Value> = [5, 25, 35, 45].iter().map(|&t| at("state", t)).collect();
        assert_eq!(states, ["idle", "start", "data", "stop_bit"].map(text), "{file}");
        assert_eq!(at("flag", 5), text("false"), "{file}");
        assert_eq!(at("flag", 35), text("true"), "{file}");

        assert_eq!(at("r", 55), Value::Real(3.25), "{file}");
        // nvc writes time zero as "0 HR"; it's tidied to "0 fs".
        assert_eq!(at("t", 5), text("0 fs"), "{file}");
        assert_eq!(at("t", 55), text("7 ns"), "{file}");

        assert_eq!(at("rec.count", 45), logic("1010"), "{file}");
        assert_eq!(at("nested.inner", 8), Value::Logic(vec![Logic::One]), "{file}");
    }
}

#[test]
fn values_exactly_at_a_change_and_between_changes() {
    let wave = open_and_load_all("types_tb.fst");
    let sl = wave.find_signal("types_tb.sl").unwrap();
    // The change at 10 ns is visible from exactly 10 ns, not before.
    assert_eq!(wave.value_at(sl, SimTime::from_femtoseconds(9_999_999)), Some(logic("U")));
    assert_eq!(wave.value_at(sl, ns(10)), Some(logic("X")));
    // After the last change the value holds.
    assert_eq!(wave.value_at(sl, ns(1_000_000)), Some(logic("H")));
    assert_eq!(wave.end_time(), ns(60));
}

#[test]
fn signals_load_only_when_asked() {
    let mut wave = open("types_tb.fst");
    let sl = wave.find_signal("types_tb.sl").unwrap();
    let bus = wave.find_signal("types_tb.bus8").unwrap();
    assert!(!wave.is_loaded(sl));
    assert_eq!(wave.value_at(sl, ns(15)), None, "not loaded yet");

    wave.load(&[sl], &CancelToken::new());
    assert!(wave.is_loaded(sl));
    assert!(!wave.is_loaded(bus), "only what was asked for");
}

#[test]
fn stop_before_loading_loads_nothing() {
    let mut wave = open("types_tb.fst");
    let all = wave.all_signals();
    let cancel = CancelToken::new();
    cancel.cancel();
    wave.load(&all, &cancel);
    assert!(all.iter().all(|&id| !wave.is_loaded(id)));
}

#[test]
fn opening_and_loading_in_the_background() {
    let (wake_sender, woken) = std::sync::mpsc::channel();
    let wake_sender_2 = wake_sender.clone();
    let opening = open_in_background(&fixture("types_tb.vcd"), move || {
        let _ = wake_sender.send(());
    });
    let mut wave = loop {
        woken.recv_timeout(Duration::from_secs(30)).expect("open took too long");
        if let Some(result) = opening.poll() {
            break result.unwrap();
        }
    };

    let ids = [wave.find_signal("types_tb.sl").unwrap(), wave.find_signal("types_tb.state").unwrap()];
    let mut loading = wave.load_in_background(&ids, move || {
        let _ = wake_sender_2.send(());
    });
    while !loading.is_finished() {
        woken.recv_timeout(Duration::from_secs(30)).expect("load took too long");
        loading.receive_into(&mut wave);
    }
    assert_eq!(loading.progress(), (2, 2));
    assert_eq!(wave.value_at(ids[1], ns(25)), Some(text("start")));
}

#[test]
fn missing_or_bad_files_give_errors() {
    let missing = Waveform::open(&fixture("no_such.fst")).err().unwrap();
    assert!(missing.to_string().contains("file not found"), "{missing}");

    // nvc's VCD from a source folder with a space in its path.
    let spaced = Waveform::open(&fixture("space_in_source_path.vcd")).err().unwrap();
    assert!(spaced.to_string().contains("path contains a space"), "{spaced}");

    // A file that isn't a waveform (one of our VHDL sources).
    let not_a_wave = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/vhdl/counter.vhd");
    assert!(Waveform::open(&not_a_wave).is_err());
}
