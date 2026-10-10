# Plan

The full plan with prompts and checklists is in
[reference/prompt-plan.md](reference/prompt-plan.md). This file tracks
progress and decisions. Update it at the end of each phase.

## Phases

| Phase | Goal | Status |
|-------|------|--------|
| 0 | Project setup: workspace, rules, CI, empty window | Done |
| 1 | NVC integration: run analyse/elaborate/run, stream output, cancel, parse diagnostics and report/assert output | Done |
| 2 | Project and libraries: `project.toml`, automatic compile order, incremental builds, OSVVM/UVVM install, first GUI screen | Done |
| 3 | Waveform engine: lazy FST/VCD loading, fast range queries, nine-value std_logic, radix formatting, benchmarks | Done |
| 4 | Waveform viewer UI: zoom/pan, std_logic colours, cursors, navigation, display options, saved layouts and auto-reload | Built; real-use checks pending (Questa comparison, a real debug session) |
| 5 | Source, diagnostics and hierarchy: source viewer, external editor, diagnostics panel, instance browser | Not started |
| 6 | Scripting and settings: command layer, console, Tcl `.do` compatibility, headless `cli build`/`cli test`, themes | Not started |
| 7 | Packaging and performance: installers, first-run wizard, profiling, robustness | Not started |
| Later | Interactive debugging (VHPI plugin, needs licensing advice) and code coverage view | Not started |

MVP = phases 0 to 4.

## Decisions

Append new decisions at the bottom: date, what was decided, and why.

- **2026-10-08: Rust + eframe/egui.** Fast custom painting for the waveform
  canvas, a single self-contained binary, and memory safety. (From the plan.)
- **2026-10-08: The shared-types crate's package name is `studio-core`**
  (folder `crates/core`). Rust already has a built-in library called `core`,
  and reusing the name causes confusing errors.
- **2026-10-08: Licence for our own code is undecided.** Revisit before
  publishing or selling. Dependency licences are still checked per CLAUDE.md.
- **2026-10-08: eframe 0.36 with its default renderer (wgpu: DirectX 12 /
  Vulkan / Metal).** Using the default is the simplest option. The OpenGL
  renderer (glow) is the fallback if wgpu misbehaves on some machine.
- **2026-10-08: Rust edition 2024, toolchain pinned in
  `rust-toolchain.toml`** so local builds and CI use the same compiler.
- **2026-10-09: NVC message styles: full for analyse/elaborate, compact for
  run.** Full style keeps hints like "did you mean A?"; compact style points
  report/assert messages at the actual statement line rather than the
  process. Details in `docs/nvc-cli-notes.md`.
- **2026-10-09: Minimum NVC version 1.23.0**, the only version whose output
  format has been checked. Older versions get a clear "please upgrade" error.
- **2026-10-09: Each nvc step runs as its own process** (not one chained
  `-a ... -e ... -r` call), so each has its own exit code, timing and parser.
- **2026-10-09: `clap` for command-line parsing** (MIT OR Apache-2.0). Pays
  off when `cli build`, `cli test` and `cli do` arrive in Phase 6.
- **2026-10-09: CI installs real NVC** (from its GitHub releases) on Linux
  and Windows. End-to-end tests skip locally when nvc is missing, but fail
  in CI (`NVC_REQUIRED=1`) so they can't be skipped by accident.
- **2026-10-09: Parser fixtures are real captured output** in
  `tests/fixtures/nvc/`, regenerated with `scripts/capture-nvc-fixtures.sh`
  after an NVC upgrade; `git diff` then shows any format change.
- **2026-10-10: Compile order comes from our own small VHDL scanner**, not
  the `vhdl_lang` crate. `nvc --print-deps` only works on files that are
  already analysed, so it can't order a first build. The scanner still finds
  units in files with syntax errors (so NVC reports the real error), adds no
  dependency, and is easy to read. `vhdl_lang` (MPL-2.0) can still be
  adopted in Phase 5 for go-to-declaration. Details in
  `docs/phase2-design.md`.
- **2026-10-10: `project.toml` schema accepted as proposed** in
  `docs/phase2-design.md`: `[project]` (name, standard, output_dir,
  analyse_args), `[[library]]` with files/globs, `[external]` search paths
  and maps, `[[testbench]]` entries. Build state (content fingerprints)
  lives in `<output_dir>/build-state.toml`.
- **2026-10-10: OSVVM/UVVM are installed with `nvc --install`**, run as a
  subprocess like every other nvc call. Tested for real with
  `nvc --install osvvm` on the development machine.
- **2026-10-10: `cli build` added in Phase 2**, so compile order can be
  tried on real projects before the GUI exists. Phase 6 then only adds
  `cli test` and `cli do`.
- **2026-10-10: Builds analyse one file per nvc process.** Gives each file
  its own pass/fail and exact incremental tracking. nvc starts fast (~20
  files in about 0.2 s here); batching runs of same-library files into one
  call is a possible later speed-up for big projects.
- **2026-10-10: A failed file doesn't stop the whole build.** Files that
  need it (directly or indirectly) are skipped; independent files are still
  compiled, so every real error is reported in one build.
- **2026-10-10: Libraries are installed by running nvc's own install
  scripts with bash, not `nvc --install`.** nvc 1.23.0 doesn't quote the
  script path on Windows, so `--install` fails whenever nvc is in
  `C:\Program Files`. Running the same script ourselves (a subprocess, as
  with nvc itself; nothing copied) works around it. bash is found
  automatically: Git for Windows' launcher, then MSYS2, or `bash` on PATH
  outside Windows. Worth reporting upstream to nvc.
- **2026-10-10: Tcllib for the OSVVM install is supplied via `TCLLIBPATH`**
  from `~/.cache/nvc-studio/tcllib/modules` (Tcllib 2.0, BSD-style Tcl
  licence; downloaded by hand for now). How end users get it is a Phase 7
  first-run wizard question.
- **2026-10-10: Installed libraries stay in nvc's default `~/.nvc/lib`,**
  and builds pass `-L ~/.nvc/lib` explicitly, so projects need no extra
  settings to use OSVVM/UVVM. (nvc finds that folder by itself only via
  the `HOME` variable, which Windows programs started normally don't
  have; found in step 4 when the GUI couldn't see OSVVM.)
- **2026-10-10: GUI structure.** nvc work (build, run, install) runs on a
  background thread that sends messages to the window through a channel
  (a FIFO between two clock domains); the window never waits on nvc.
  Logic lives in egui-free modules (`state`, `worker`, `session`,
  `recent`) with tests, including real-nvc runs; `ui.rs` only draws.
  Output and Problems fill the main area until the source viewer arrives
  in Phase 5. `app path/to/project` opens a project from the command line.
- **2026-10-10: Waveforms are read with the `wellen` crate** (BSD-3-Clause,
  used by the Surfer viewer; FST, VCD and GHW). Only the `wave` crate
  depends on it, so it could be replaced without touching anything else.
  Details and timings in `docs/phase3-design.md`.
- **2026-10-10: Every run writes an FST waveform by default** to
  `<output_dir>/waves/<testbench>.fst`; `waves = false` on a testbench
  turns it off, and `wave_args` passes extra options (`--include`,
  `--exclude`, `--dump-arrays`). `--dump-arrays` stays off by default
  (nvc's default: smaller files).
- **2026-10-10: Zoomed-out views use a binary search per pixel column**,
  not a precomputed summary pyramid. A pyramid is only added if the
  benchmark shows it's needed.
- **2026-10-10: Benchmarks are an on-demand timing program**, not the
  `criterion` crate, to avoid a large set of extra dependencies.
- **2026-10-10: `cli build --run <testbench>`** builds, then runs one
  testbench and reports its waveform. Added early (rather than waiting for
  Phase 6's `cli test`) so realistic waveforms can be made headlessly by
  `scripts/real-waveforms.sh`.
- **2026-10-10: Stand-ins for "your largest real waveform"** until one is
  available: UVVM's UART demo (built through our project system) and
  OSVVM's AXI4 demo tests (through OSVVM's scripts), both made by
  `scripts/real-waveforms.sh`, plus the generated benchmark waveform.
- **2026-10-10: Phase 3 closed without the Questa comparison**; the user
  will compare against Questa once the viewer exists (Phase 4).
- **2026-10-10: The waveform viewer is a "Waves" tab** next to Output and
  Problems: signal tree | names + values | waves, ruler on top, cursor
  bar at the bottom. After a successful run its waveform opens and the
  Waves tab is shown. Other waveforms open with File → Open waveform… or
  by dropping a .fst/.vcd/.ghw file on the window.
- **2026-10-10: Wave view layouts are saved automatically** (about a
  second after changes stop, and on exit) as readable TOML: for project
  runs in `<project>/layouts/<testbench>.toml` (beside `project.toml`, so
  it can be kept in version control, unlike `build/`), for other files as
  `<waveform>.layout.toml` next to the file. Signals are stored by path;
  ones missing after a design change are skipped and reported. Nothing is
  written just for opening a file and looking.
- **2026-10-10: Re-running reloads the waveform keeping the whole setup**
  (signals, groups, names, radix, colours, zoom, cursors, markers). The
  app also reloads when the open waveform file changes on disk and then
  stays unchanged for a second (e.g. re-simulated from the command line).
  nvc rewriting a waveform the app has open works on Windows (tested).
- **2026-10-10: Wave view extras requested during Phase 4:** dragging
  cursors and markers, live edge snapping with a hover hint, and
  multi-row selection (Ctrl/Shift+click) to group, remove or move rows
  together.
