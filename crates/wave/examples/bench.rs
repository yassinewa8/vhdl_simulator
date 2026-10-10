//! Times opening a waveform, loading signals and the viewer's queries.
//!
//! ```text
//! cargo run --release -p wave --example bench                    # generate and time a 50 ms run
//! cargo run --release -p wave --example bench -- --stop-time 5ms # a smaller one
//! cargo run --release -p wave --example bench -- --file some.fst # time any waveform
//! ```
//!
//! Without `--file`, it simulates tests/vhdl/bench/bench_tb.vhd with nvc
//! (about 4 changes per 10 ns of simulated time) into target/bench/, and
//! reuses that file next time. Always use `--release`: a debug build is
//! many times slower and says nothing about real performance.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use studio_core::{CancelToken, Logic, SimTime, Value};
use wave::{SignalId, SignalKind, Waveform};

/// One frame at 60 frames per second: what a per-frame query must beat.
const FRAME_BUDGET: Duration = Duration::from_micros(16_667);
/// Columns in a typical wave display.
const PIXELS: usize = 2_000;
/// How many times each query is repeated to get a steady timing.
const REPEATS: u32 = 10;
/// Signals to load from a user-supplied file (real designs have thousands).
const MAX_SIGNALS_FROM_FILE: usize = 20;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();

    let file = match option("--file") {
        Some(file) => PathBuf::from(file),
        None => generate(&option("--stop-time").unwrap_or_else(|| "50ms".to_string())),
    };
    println!("Waveform: {} ({})", file.display(), file_size(&file));

    let started = Instant::now();
    let mut wave = Waveform::open(&file).unwrap_or_else(|error| panic!("{error}"));
    let all = wave.all_signals();
    println!(
        "Open (tree of {} signals + change times): {}",
        all.len(),
        show(started.elapsed())
    );
    let end = wave.end_time();
    println!("Simulated time: {end}\n");

    let signals: Vec<SignalId> = all.into_iter().take(MAX_SIGNALS_FROM_FILE).collect();
    println!(
        "{:<28} {:>10} {:>9} | {:>11} {:>11} {:>11} {:>11} {:>11}",
        "signal", "changes", "load", "zoomed in", "zoomed out", "next chg", "search", "value at"
    );
    println!("{:<28} {:>10} {:>9} | {:>11} {:>11} {:>11} {:>11} {:>11}", "", "", "", "(1 us)", "(all)", "", "(worst)", "");

    let mut slowest_per_frame = Duration::ZERO;
    for id in signals {
        let info = wave.signal(id);
        let started = Instant::now();
        wave.load(&[id], &CancelToken::new());
        let load = started.elapsed();
        let change_count = wave.changes(id, SimTime::ZERO..SimTime::from_femtoseconds(u64::MAX)).unwrap().changes.len();

        let middle = SimTime::from_femtoseconds(end.femtoseconds / 2);
        let zoomed_in = middle..SimTime::from_femtoseconds(middle.femtoseconds + 1_000_000_000);
        let whole_run = SimTime::ZERO..end;

        // What one frame of the viewer does for this signal: the visible
        // changes plus the per-pixel summary.
        let zoomed_in_time = average(|| {
            wave.changes(id, zoomed_in.clone());
            wave.summary(id, zoomed_in.clone(), PIXELS);
        });
        let zoomed_out_time = average(|| {
            wave.summary(id, whole_run.clone(), PIXELS);
        });
        let next_change_time = average(|| {
            wave.next_change(id, middle);
        });
        let value_at_time = average(|| {
            wave.value_at(id, middle);
        });
        // Worst case for the value search: a value that never occurs, so
        // every change from the start is checked.
        let missing = value_that_never_occurs(info.kind);
        let started = Instant::now();
        assert_eq!(wave.next_equal(id, SimTime::ZERO, &missing), None);
        let search_time = started.elapsed();

        slowest_per_frame = slowest_per_frame.max(zoomed_in_time).max(zoomed_out_time);
        println!(
            "{:<28} {:>10} {:>9} | {:>11} {:>11} {:>11} {:>11} {:>11}",
            short(&info.path, 28),
            change_count,
            show(load),
            show(zoomed_in_time),
            show(zoomed_out_time),
            show(next_change_time),
            show(search_time),
            show(value_at_time)
        );
    }

    println!(
        "\nSlowest per-frame query: {} ({:.1}% of a 60 fps frame).",
        show(slowest_per_frame),
        100.0 * slowest_per_frame.as_secs_f64() / FRAME_BUDGET.as_secs_f64()
    );
}

/// Simulates bench_tb for `stop_time` with nvc, unless done before.
fn generate(stop_time: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("target").join("bench");
    let file = dir.join(format!("bench_tb_{stop_time}.fst"));
    if file.is_file() {
        println!("Reusing {} (delete it to simulate again)", file.display());
        return file;
    }
    std::fs::create_dir_all(&dir).unwrap();
    let nvc = nvc::locate_nvc(None).unwrap_or_else(|error| panic!("{error}"));
    let source = root.join("tests/vhdl/bench/bench_tb.vhd");
    println!("Simulating bench_tb for {stop_time} with nvc...");
    let started = Instant::now();
    let status = Command::new(&nvc.program)
        .current_dir(&dir)
        .args(["--std=2008", "--work=work:work", "-a"])
        .arg(&source)
        .args(["-e", "bench_tb", "-r", &format!("--stop-time={stop_time}")])
        .arg(format!("--wave={}", file.file_name().unwrap().to_string_lossy()))
        .arg("bench_tb")
        .status()
        .unwrap();
    assert!(status.success(), "nvc failed");
    println!("Simulated in {}", show(started.elapsed()));
    file
}

/// Average time of `query` over [`REPEATS`] runs.
fn average(mut query: impl FnMut()) -> Duration {
    let started = Instant::now();
    for _ in 0..REPEATS {
        query();
    }
    started.elapsed() / REPEATS
}

fn value_that_never_occurs(kind: SignalKind) -> Value {
    match kind {
        SignalKind::Logic { width } => Value::Logic(vec![Logic::DontCare; width as usize]),
        SignalKind::Text => Value::Text("no such value".to_string()),
        SignalKind::Real => Value::Real(f64::NAN), // NaN never equals anything
    }
}

fn show(duration: Duration) -> String {
    let micros = duration.as_secs_f64() * 1e6;
    if micros < 1_000.0 {
        format!("{micros:.1} us")
    } else if micros < 1_000_000.0 {
        format!("{:.1} ms", micros / 1_000.0)
    } else {
        format!("{:.2} s", micros / 1_000_000.0)
    }
}

fn file_size(path: &Path) -> String {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) as f64;
    if bytes < 1e6 {
        format!("{:.0} KB", bytes / 1e3)
    } else {
        format!("{:.1} MB", bytes / 1e6)
    }
}

/// The end of `text`, at most `width` characters, for the table.
fn short(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        text.to_string()
    } else {
        let tail: String = text.chars().skip(count - (width - 1)).collect();
        format!("…{tail}")
    }
}
