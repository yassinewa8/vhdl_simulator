# Architecture and handover notes

For a new Claude Code session (or anyone) picking up this project. Read
this after `CLAUDE.md` and `docs/PLAN.md`. It explains how the code is put
together, how to test it, and the traps already found, so changes and
fixes are quick and safe.

Last updated 2026-10-10, at the end of Phase 4 (MVP built).

## 1. Where things stand

- Phases 0–4 are built: project files with automatic compile order and
  incremental builds, OSVVM/UVVM install, a GUI to build/run with live
  output and problems, a waveform engine, and a full waveform viewer.
- **Next is Phase 5** (source viewer, diagnostics, hierarchy) — see
  `docs/reference/prompt-plan.md`. The plan recommends the user first uses
  the tool on real work for a week or two and lists annoyances; ask for
  that list before starting Phase 5.
- Pending user checks for Phase 4: compare colours/values with Questa,
  and debug a real issue with the viewer.
- The user's process: each phase starts with research and a short design
  proposal (`docs/phaseN-design.md`) with questions; decisions go into
  `docs/PLAN.md`; then the phase is built in numbered steps. At the start
  of each step say briefly what you'll do; after each step run build,
  test and clippy, commit, and **wait for the user's OK**. Push only when
  asked. The user is an FPGA engineer, not a software engineer: explain
  in plain terms (hardware analogies help) and ask before big decisions.

## 2. Crates and what lives where

```
studio-core  shared types (no deps on other crates)
   ↑
  nvc        runs nvc as a subprocess, parses output, installs libraries
   ↑
 project     project.toml, scanner, compile order, incremental build, run testbench
 wave        waveform reading (wellen) and queries        ← only crate using wellen
   ↑
 app (GUI)   eframe/egui window          cli   headless command-line tool
```

Rules (from `CLAUDE.md`): GUI code only in `app`; every other crate is
testable without a GUI. NVC is GPL: only ever run it as a process.

### studio-core (`crates/core`)
| File | Holds |
|---|---|
| `time.rs` | `SimTime` (femtoseconds, `u64`), parse/display (`"15 ns"`). |
| `severity.rs`, `diagnostic.rs` | `Severity`, `Diagnostic`, `SimMessage`, `Location`. |
| `value.rs` | `Logic` (std_logic's 9 values), `Value` (Logic vector MSB-first / Text / Real). |
| `radix.rs` | `Radix`, `format_value`/`format_logic` (unknown-safe: `X5`, `?`), `default_radix` from VHDL type name, `format_vhdl_time`. |
| `parse.rs` | `parse_logic_value` — what the user typed (`A5`, `0x1F`, `-3`, `X`) → bits. Inverse of `radix.rs`. |
| `cancel.rs` | `CancelToken` (Stop flag), re-exported by `nvc`. |

### nvc (`crates/nvc`)
| File | Holds |
|---|---|
| `locate.rs` | Find nvc (PATH, then `C:\Program Files\NVC`), check version ≥ 1.23.0. |
| `options.rs` | `NvcOptions` → argument lists per step (`-L`, `--map`, extra args). |
| `runner.rs` | `run_process(_with_env)`: spawn, stream lines from stdout/stderr via reader threads, cancel by killing. |
| `diagnostics.rs` / `sim_messages.rs` | Parsers: "full" style (analyse/elaborate) and "compact" style (run). |
| `steps.rs` | `Nvc::{analyse, elaborate, run}` → `StepReport`. |
| `format.rs` | Parsed output → text (CLI and GUI log). |
| `install.rs` | OSVVM/UVVM install by running nvc's own `libexec/nvc/install-*.sh` with bash (see gotchas). |

### project (`crates/project`)
| File | Holds |
|---|---|
| `config.rs` | `ProjectConfig` (serde) = `project.toml` schema; checks with clear messages. |
| `files.rs` | Expand `files` globs per library. |
| `scanner.rs` | Token-based VHDL scanner: units defined, names referenced. |
| `order.rs` | Dependency graph, topological compile order, cycle reporting. |
| `state.rs` | Content fingerprints in `build/build-state.toml`; what needs recompiling. |
| `build.rs` | `Project`, `plan_build`, `run_build` (one nvc process per file), `run_testbench` (writes `build/waves/<tb>.fst`). |
| `new_project.rs` | "New project" from a folder of VHDL files. |

### wave (`crates/wave`)
| File | Holds |
|---|---|
| `waveform.rs` | `Waveform::open` (tree + change times), `load` (lazy per signal), `value_at`, `SignalView` (indexed changes). `ScopeId`/`SignalId`/`SignalInfo`. |
| `query.rs` | Viewer queries: `changes`, `summary` (per-pixel Quiet/One/Many), `next/prev_change`, `next/prev_equal`. Binary search everywhere. |
| `background.rs` | `open_in_background`, `load_in_background` (thread + channel, progress, Stop between signals). |
| `convert.rs` | wellen → our types; `value_matches` compares in place (fast search). |
| `examples/bench.rs` | Timing program (`cargo run --release -p wave --example bench`). |

### app (`crates/app`) — the GUI
Pattern: **logic in egui-free modules (tested), drawing in `ui.rs` /
`wave_ui.rs`.** Background work never blocks the window.

| File | Holds |
|---|---|
| `main.rs` | Starts eframe. `app <project>` or `app <file.fst>`. |
| `state.rs` | `AppState`: project, nvc, jobs, session, `waves`; actions (open project, start job, poll, open waveform after a run). Owns the *waker* (egui `request_repaint`). |
| `worker.rs` | `Job` (Build / Run / Install) on a thread; `WorkerMessage`s through a channel; `JobOutcome` (incl. `wave_file`). |
| `session.rs` | Output log, Problems list, progress line (from worker messages). |
| `recent.rs` | Recent projects list (saved via eframe storage). |
| `ui.rs` | Main window: menus, toolbar, project file tree, Output/Problems/Waves tabs. |
| `wave_state.rs` | `WaveState`: open waveform, `list` of rows, selection, cursors, timeline, navigation (`step_edge`, `find_value`), tree search, layout autosave/reload/file watching. |
| `wave_list.rs` | `SignalList`: rows = signals, dividers, one-level groups; move/group/remove (single and multiple rows), drop targets. Tracks signals by id when moving several. |
| `timeline.rs` | Time↔pixel maths: zoom about a point, pan, fit, ruler ticks, pixel-precise time text. |
| `cursors.rs` | Two cursors + markers, snapping rule, drag targets, 1/Δ frequency text. |
| `wave_style.rs` | How values look (`Look`, `Level`), `Palette` (dark/light), per-signal `WaveColour`. |
| `layout.rs` | Wave view layout ↔ TOML (signals by path), capture/restore. |
| `wave_ui.rs` | Draws the Waves tab: tree + search, names/values columns, ruler, waves (per-pixel summary), cursors, menus, all mouse/keyboard handling. |

### cli (`crates/cli`)
`cli run <files> --top tb`, `cli build [project] [--run tb] [--dry-run] [--force]`,
`cli install [package]`.

## 3. How the main flows work

**Build and run (GUI):** Run button → `AppState::start_job(Job::Run)` →
`RunningJob::start` spawns a thread running `worker::run_job` → it calls
`project::plan_build` / `run_build` / `run_testbench`, sending each
`BuildEvent` as a `WorkerMessage` and calling the waker → each frame
`AppState::poll` drains messages into `Session` → on `Finished`, the plan
is refreshed and, after a successful run, `open_run_waveform` reloads or
opens `build/waves/<tb>.fst` in the Waves tab.

**Waveform view:** `WaveState::open_with_layout` starts
`wave::open_in_background`; `poll` receives the `Waveform`, restores the
pending layout, and starts `load_in_background` for shown signals. Each
frame `wave_ui::wave_canvas` asks `Waveform::summary(id, visible, pixels)`
per visible row — cost depends on pixels, not on the number of changes
(21 M changes draw in ~1.6 ms in release).

**Layouts:** `Layout::capture(&WaveState)` / `restore`. Autosaved one
second after the last change (`WaveState::autosave`, called from
`AppState::poll`), on exit (`on_exit`), and before opening another file.
Project runs: `<project>/layouts/<tb>.toml`; other files:
`<file>.layout.toml`. `reload()` captures the layout in memory and reopens.

**Wakers:** egui only redraws when asked. Background threads call a
`Fn() + Send + Sync` waker (egui's `request_repaint`). Pass the current
waker explicitly to anything that starts background work (`open_with_layout`,
`reload`, `check_file`) — storing an old one broke tests once.

## 4. Testing

Always before saying something is done (from `CLAUDE.md`):

```
cargo build --workspace
cargo test --workspace --no-fail-fast        # NVC_REQUIRED=1 to forbid skipping
cargo clippy --workspace --all-targets -- -D warnings
```

- Tests that need nvc skip without it unless `NVC_REQUIRED=1` (CI sets
  it). OSVVM test skips unless OSVVM is installed (`OSVVM_REQUIRED=1`).
- `cargo test -p nvc --test real_install -- --ignored` runs a real OSVVM
  install (downloads).
- Fixtures: `tests/fixtures/nvc/` (parser output) and
  `tests/fixtures/waves/` (FST/VCD). Regenerate with
  `bash scripts/capture-nvc-fixtures.sh`; never hand-edit.
- Test designs: `tests/vhdl/` (see its README). `waves/resolution_tb.vhd`
  is checked against the IEEE 1164 resolution table.
- Realistic waveforms: `bash scripts/real-waveforms.sh` (UVVM UART demo,
  OSVVM AXI4 tests → `target/real-waves/`). Benchmark:
  `cargo run --release -p wave --example bench [-- --file x.fst]`.
- GUI: verify visually with `scripts/gui-test/` (screenshots + UI
  Automation + mouse); see its README. Clippy sometimes finishes too fast
  to have rechecked — `touch` a changed file and run it again.
- CI (GitHub Actions, Linux + Windows) reports failing tests as an
  annotation ("Failed tests") readable without admin access:
  `https://api.github.com/repos/yassinewa8/vhdl_simulator/check-runs/<job id>/annotations`.
  The anonymous API allows 60 requests/hour — don't poll often.

## 5. Gotchas already found (don't rediscover them)

**Windows / tools**
- Never edit files with Windows PowerShell 5 `Get-Content`/`Set-Content`:
  it reads UTF-8 as the ANSI code page and garbles `−`, `…`, `→`. Use the
  edit tools or Python (`newline=""` to keep line endings).
- Bash heredocs for Python: backslashes get mangled (`\\` → `\`); put
  longer scripts in a file instead.
- `rm -rf "$VAR/..."` is blocked by a safety check; use literal paths.
- Git for Windows has `core.autocrlf=true`. `tests/fixtures/**` is
  `-text` in `.gitattributes`; `.gitignore` ignores `*.fst`/`*.vcd` except
  `tests/fixtures/waves/*`.

**nvc**
- nvc doesn't create a library's parent folder; create it first.
- `--std` must match between analyse and elaborate.
- `--map` to a non-existent folder warns; only map existing libraries.
- nvc finds `~/.nvc/lib` only via `HOME`, which Windows GUI programs
  don't have: builds pass `-L <USERPROFILE>/.nvc/lib` explicitly.
- `nvc --install` doesn't quote the script path on Windows (fails under
  `C:\Program Files`): we run `libexec/nvc/install-PKG.sh` with bash
  ourselves (Git for Windows' `bin\bash.exe` launcher, or MSYS2), set
  `NVC`, `TCLLIBPATH` (Tcllib for OSVVM, kept in
  `~/.cache/nvc-studio/tcllib`), and `core.autocrlf=false` via
  `GIT_CONFIG_*` (UVVM's scripts break on CRLF file names).
- nvc writes the source path unquoted into waveforms: a space in it makes
  the **VCD** unreadable (FST is fine). OSVVM's `SetSaveWaves` also breaks
  on spaces in the output path.
- OSVVM's `EndOfTestReports` needs OSVVM's Tcl flow; use `ReportAlerts`.
- Enums, booleans and `time` arrive in waveforms as text (enum names
  lower case; time zero as `"0 HR"`).

**Rust / cross-platform**
- `join_paths` with `C:` fails on Linux (`:` is the PATH separator), and
  `bin/..` only resolves on Linux if `bin` exists — take parents directly.
  CI on Linux catches these; run with `--no-fail-fast`.

**egui 0.36**
- Panels: `egui::Panel::left/top/bottom(id).show(ui, …)`; app entry is
  `App::ui`. Ctrl+scroll arrives as `zoom_delta()`.
- The default font lacks some symbols (●, ✕, ▼): use other characters or
  paint shapes (group triangles are painted).
- `ui.dnd_drag_source` swallows clicks on its contents; for an item that
  is both double-clickable and draggable use
  `response.interact(Sense::click_and_drag())` + `dnd_set_drag_payload`.
- Menus: call `ui.close()` after an action.

## 6. Ideas and loose ends noted along the way

- Batch consecutive same-library files into one nvc analyse call (speed).
- CI could install Tcl/Tcllib and OSVVM to run the OSVVM test.
- The user may want the wave palette closer to Questa's.
- `actions/checkout@v4` warns about Node 20 retirement; bump to v5.
- Possible later: report nvc's unquoted-path issues upstream.
