# Plan

The full plan with prompts and checklists is in
[reference/prompt-plan.md](reference/prompt-plan.md). This file tracks
progress and decisions. Update it at the end of each phase.

## Phases

| Phase | Goal | Status |
|-------|------|--------|
| 0 | Project setup: workspace, rules, CI, empty window | In progress |
| 1 | NVC integration: run analyse/elaborate/run, stream output, cancel, parse diagnostics and report/assert output | Not started |
| 2 | Project and libraries: `project.toml`, automatic compile order, incremental builds, OSVVM/UVVM install, first GUI screen | Not started |
| 3 | Waveform engine: lazy FST/VCD loading, fast range queries, nine-value std_logic, radix formatting, benchmarks | Not started |
| 4 | Waveform viewer UI: zoom/pan, std_logic colours, cursors, navigation, display options, saved layouts and auto-reload | Not started |
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
