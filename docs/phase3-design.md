# Phase 3 design proposal

Status: **proposed, waiting for the user's answers** to the questions at the
end. Written 2026-10-10 after investigating; nothing in Phase 3 is built yet.
When decisions are made, record them in `docs/PLAN.md` (Decisions).

## Findings

Tested with NVC 1.23.0 on a testbench using every type the viewer cares
about, and on a 50 ms run of a clock plus 32-bit counter (15 million
transitions).

### What NVC writes

- `nvc -r --wave=FILE` writes **FST** by default; `--format=vcd` gives VCD.
  nvc doesn't write GHW. Other options: `--include=GLOB` / `--exclude=GLOB`
  to filter signals, `--dump-arrays[=N]` to include arrays of composite
  types (left out by default, with a note in the output).
- Every signal carries its **VHDL type name** (`STD_LOGIC`, `SIGNED`,
  `UNSIGNED`, `STATE_T`, `INTEGER`, ...).
- **All nine std_logic values** are kept (`U X 0 1 Z W L H -`).
- **Enums, booleans and `time` are written as text**: `"idle"`, `"start"`
  (lower case), `"true"`, `"7 NS"`. Time zero is written `"0 HR"`.
- Integers are 32-bit vectors, reals are reals, records become scopes.
- Timescale is 1 fs.
- FST is tiny for regular activity: 15 M transitions took 1.7 s to
  simulate and gave a **179 KB** file.

### The `wellen` crate

- Version 0.25.6, **BSD-3-Clause**. Its 39 dependencies are all permissive
  (MIT / Apache-2.0 / BSD / Zlib / Unicode-3.0); 6 are already in our tree.
  Made for the Surfer waveform viewer; reads FST, VCD and GHW.
- Gives us everything listed above: hierarchy, VHDL type names, nine-value
  bit vectors, strings for enums.
- Loads in two stages, which is exactly the "hierarchy first, signals
  lazily" the plan asks for:

  | Step (release build, 15 M-transition file) | Time |
  |---|---|
  | Header: all scopes and signals | 98 ms |
  | Body (time table, 10 M entries) | 9 ms |
  | Load one signal with 10 M changes | ~0.4 s |
  | "Changes between t and t + 1 µs" (binary search) | **7–11 µs** |

- Loading a single signal can't be cancelled or report progress part-way;
  for VCD the body read can report progress. So loading is cancellable
  *between* signals, and progress is "signals loaded so far".
- Memory: the time table is 8 bytes per distinct change time (80 MB for
  this file). Fine for now; worth watching on multi-gigabyte dumps.

## Proposal

### Use `wellen` for reading files (recommended)

Writing our own FST reader would be weeks of work for no gain. The `wave`
crate wraps `wellen` so nothing else in the project depends on it: if we
ever replace it, only `wave` changes.

### Values in `studio-core`

The core crate's doc already says signal values arrive in Phase 3:

```rust
enum Logic { U, X, Zero, One, Z, W, L, H, DontCare }   // std_logic's nine values

enum Value {
    Logic(Vec<Logic>),   // std_logic, std_logic_vector, signed, unsigned, bit, integer
    Text(String),        // enums, booleans, time (as NVC writes them)
    Real(f64),
}
```

Formatting helpers (pure functions, tested):

- binary, hex, octal, unsigned decimal, signed decimal, ASCII
- hex/decimal of a vector containing U/X/Z: show the letter when a whole
  digit is that value (`X`, `Z`), otherwise `?` per digit (so `0xA?`),
  never a wrong number
- default radix from the VHDL type: `SIGNED` and `INTEGER` → signed,
  `UNSIGNED` and vectors → hex, single bits → the bit
- enums shown as their name; `"0 HR"` shown as `0 fs`

### The `wave` crate's API (what the viewer calls each frame)

```rust
let wave = Waveform::open(path)?;            // header + time table only
wave.scopes() / wave.signals_in(scope)       // tree with name, VHDL type, width
wave.load(&[id, ...], &cancel, progress)?    // lazy, only displayed signals
wave.value_at(id, time)
wave.changes(id, t0..t1)                     // value just before t0 + every change in range
wave.summary(id, t0..t1, pixels)             // per pixel: Nothing | One(value) | Many(count)
wave.next_change(id, t) / prev_change(id, t)
wave.next_equal(id, t, &value) / prev_equal(id, t, &value)
```

Background loading (with progress and Stop) follows the same pattern as
the GUI's worker: a thread plus a channel, with the logic testable without
egui.

### Zoomed-out views: search per pixel first, not a precomputed pyramid

For each pixel column, two binary searches give how many changes fall in
it (one change → draw it; several → draw a "many changes" block). At
2,000 pixels × log₂(10 M) ≈ 24 steps, that's about 50,000 cheap steps per
signal per frame, well under a millisecond. A precomputed summary pyramid
(the classic approach) costs memory and load time; we only build one if
the benchmark shows we need it.

### Runs write FST by default

`project` passes `--wave=<output_dir>/waves/<testbench>.fst` on every
run, so Phase 4 can open the latest waveform automatically. Per-testbench
settings in `project.toml`:

```toml
[[testbench]]
top = "uart_tx_tb"
waves = true                    # default true; false = no waveform (faster)
wave_args = ["--dump-arrays"]   # optional extra options: --include, --exclude, ...
```

### Benchmarks

A generator testbench in `tests/vhdl/bench/` (clock + counter + a wide
bus + an enum, length set by a generic so it can reach 10 M+ transitions),
and a timing program that prints load and query times for zoomed-in and
fully zoomed-out windows. Run on demand (`cargo run --release -p wave
--example bench`), not in every test run, as it takes several seconds.

### New dependencies

`wellen` (BSD-3-Clause) and its permissive dependencies, listed above.

### Steps (commit after each, wait for OK)

1. Values and formatting in `studio-core` (no waveform files yet).
2. `wave` crate: open, hierarchy, lazy loading, background loader with
   progress and cancel.
3. Query API: changes in range, summary per pixel, next/previous change,
   next/previous equal.
4. Runs write FST by default (`project`), benchmark testbench and timing
   program.

## Open questions for the user

1. Use `wellen` (BSD-3-Clause) for reading FST/VCD? (Recommended.)
2. Waveforms on by default for every run, saved as
   `build/waves/<testbench>.fst`, with `waves = false` to turn off per
   testbench? Should `--dump-arrays` be on by default (arrays of records
   and of vectors appear, but bigger files)?
3. Start with per-pixel binary search for zoomed-out views, adding a
   precomputed summary only if the benchmark needs it? (Recommended.)
4. Benchmarks as an on-demand timing program rather than the `criterion`
   benchmarking crate (more precise statistics, but a sizeable set of
   extra dependencies to check)? (Recommended.)
5. Done-when needs "your largest real waveform": can you put one
   somewhere I can read it (an FST or VCD from nvc, or a VCD from
   Questa)? Also, a testbench whose U/X/Z behaviour you know well, for
   the "values come through correctly" check.
