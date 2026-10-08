# NVC GUI & Waveform Viewer: Claude Code Prompt Plan

Oct 8, 2026 · @Walid

## How to use this plan

Run one phase per Claude Code session, in order, and don't start the next phase until the current one meets its "done when" checklist. Each phase below has a goal, a prompt to paste into Claude Code, and a checklist you verify yourself.

The working loop for every phase:

1. Start the session by telling Claude Code to read `CLAUDE.md` and `docs/PLAN.md` (created in Phase 0).
2. Paste the phase prompt. Ask Claude to propose a design first and wait for your approval before writing code.
3. Let it implement in small steps, running `cargo build` and `cargo test` after each one.
4. Test the result yourself on a real VHDL project of yours, not just toy examples.
5. Commit to git when the checklist passes, then ask Claude to update `docs/PLAN.md` with what was done and any decisions made.

Habits that save you the most pain:

- **Commit before every big change.** If Claude takes a wrong turn, `git reset` is cheaper than untangling it.
- **Keep sessions focused.** One phase or sub-feature per session. Start a fresh session rather than letting one run for hours.
- **Ask "why" on architecture, not on every line.** You need to understand how the pieces connect, not every function.
- **Your VHDL expertise is the test oracle.** When a waveform or error message looks wrong, trust your instinct and make Claude prove it right.

## Tech stack and architecture

Use Rust with egui (via eframe) for the GUI. A waveform viewer lives or dies on drawing millions of transitions smoothly, and egui's custom painting is well suited to that while still giving you ordinary panels, trees and buttons.

Why this stack over the alternatives:

- **Rust's compiler catches memory and threading bugs** that would otherwise be hard for you to debug, and Claude Code writes it well.
- **Useful crates already exist in Rust**: wellen for reading FST/VCD waveforms (used by the Surfer viewer) and vhdl\_lang from the rust\_hdl language server for parsing VHDL. Check both licences in the relevant phase.
- **One self-contained binary** on Windows, Linux and macOS, with no runtime to install.
- C++ with Qt is the mature alternative, but Qt's licensing and C++'s memory pitfalls add risk for you. A web-based UI (Tauri) looks great but needs more work to make the waveform canvas fast.

&#91;embedded content: architecture · Rust workspace crates and the NVC process boundary\]

Everything above the NVC boundary is your code and your licence. The app reads waveform data from the wave crate, which loads the FST files NVC writes; the core crate (shared types) is used by all of them and left out for clarity.

## Licensing guardrails

The GUI talks to NVC only as a separate program, so the GUI stays your own code under whatever licence you choose. NVC is GPLv3; linking it into the GUI or copying its source would make the combined work GPL too. This is the general picture, not legal advice; check with a lawyer before selling.

Rules to put in `CLAUDE.md` so Claude Code follows them every session:

- Run `nvc` as a subprocess only. Never link `libnvc`, never copy NVC source into this repo.
- Exchange data through command-line arguments, stdout/stderr, and files NVC writes (waveforms, libraries).
- Check the licence of every Rust crate before adding it. Prefer MIT, Apache-2.0, BSD or MPL-2.0. Flag any GPL or EUPL dependency before adding it.
- If you later bundle the `nvc` binary in an installer, ship its licence text and a link to its source alongside it.
- Interactive debugging that loads a plugin into the NVC process (see the later section) is a grey area. Keep it out of anything you sell until you've had advice.

## Feature scope

The MVP is phases 0 to 4: open a project, compile, see readable errors, run, and browse waveforms fast. Everything after that is what turns it into a tool you'd prefer over Questa.

| Feature | Area | Phase | Priority |
| --- | --- | --- | --- |
| Run analyse / elaborate / run in the background, with cancel | NVC integration | 1 | MVP |
| Parse NVC errors and warnings into file, line, message | NVC integration | 1 | MVP |
| Project file (TOML) listing sources, libraries, top-level, VHDL standard | Project | 2 | MVP |
| Compile order handled automatically, incremental recompile | Project | 2 | MVP |
| One-click install of OSVVM, UVVM and vendor libraries via `nvc --install` | Project | 2 | MVP |
| Multiple testbenches per project, pick one and run | Project | 2 | MVP |
| Load FST and VCD waveforms, lazily, for multi-GB files | Waveform engine | 3 | MVP |
| Smooth zoom/pan with millions of transitions | Waveform viewer | 4 | MVP |
| Signal tree, search/filter, drag signals into the view | Waveform viewer | 4 | MVP |
| Radix per signal (bin, hex, signed, unsigned), buses, std\_logic colours for U/X/Z/W/- | Waveform viewer | 4 | MVP |
| Cursors, markers, time deltas between cursors | Waveform viewer | 4 | MVP |
| Jump to next/previous edge or value change | Waveform viewer | 4 | MVP |
| Enum and record display (state machine names, not numbers) | Waveform viewer | 4 | MVP |
| Analogue-style display for integer/real signals | Waveform viewer | 4 | Later |
| Save and restore wave layouts per testbench | Waveform viewer | 4 | MVP |
| Auto-reload waves after re-running the simulation, keeping layout and cursors | Waveform viewer | 4 | MVP |
| Source viewer with VHDL highlighting, click error to jump to line | Editor | 5 | MVP+ |
| Open file in your own editor (VS Code etc.) at the right line | Editor | 5 | MVP+ |
| Design hierarchy browser, click instance to see its signals | Editor | 5 | MVP+ |
| Diagnostics panel with filters and assert/report output | Editor | 5 | MVP+ |
| Command console and scripting | Scripting | 6 | Later |
| ModelSim-style `.do` compatibility for common commands | Scripting | 6 | Later |
| Headless CLI mode for CI using the same project file | Scripting | 6 | Later |
| Light/dark themes, keyboard shortcuts, settings | Polish | 6 | Later |
| Installers for Windows, Linux, macOS | Packaging | 7 | Later |
| Breakpoints, stepping, live waveforms during simulation | Debugging | Later | Later |
| Code coverage view | Debugging | Later | Later |

## Phase 0: Project setup

Goal: an empty Rust workspace that builds, with the rules and plan files Claude Code reads every session. Expect one session.

```text
I'm an FPGA engineer building a desktop GUI and waveform viewer on top of the NVC VHDL simulator. I know VHDL and simulators deeply but I'm not an experienced software engineer, so explain architectural decisions in plain terms and ask before making big ones.

Set up a Rust Cargo workspace with these crates:
- core: shared types (diagnostics, time values, signal values)
- nvc: runs the nvc binary as a subprocess and parses its output
- project: project file loading/saving and build orchestration
- wave: waveform loading and querying (no GUI code)
- app: the desktop GUI using eframe/egui
- cli: a headless command-line tool reusing project + nvc

Rules (write these into CLAUDE.md at the repo root):
- NVC is GPLv3. Only ever run it as a subprocess. Never link libnvc or copy NVC source.
- Check the licence of every new crate dependency and tell me before adding anything that isn't MIT, Apache-2.0, BSD or MPL-2.0.
- GUI code lives only in app. All other crates must be testable without a GUI.
- Every feature gets tests. Run cargo build, cargo test and cargo clippy before saying a task is done.
- Prefer simple, readable code over clever code. I need to be able to follow it.

Also:
- Create docs/PLAN.md with the phase list I'll give you, and a 'Decisions' section we'll append to.
- Create a tests/vhdl/ folder for small VHDL test designs.
- Add a GitHub Actions workflow that runs build, test and clippy on Linux and Windows.
- Add a .gitignore and a README with build instructions.
- Make the app crate open an empty window titled 'NVC Studio' (placeholder name).

Propose the layout first and wait for my OK before writing files.
```

Done when:

- [ ] `cargo run -p app` opens an empty window
- [ ] `cargo test` passes and CI is green
- [ ] `CLAUDE.md` and `docs/PLAN.md` exist and say what you expect
- [ ] First commit pushed

## Phase 1: NVC integration layer

Goal: a library that runs NVC's analyse, elaborate and run steps, streams output, and turns errors into structured data. This is the foundation everything else sits on. Expect two or three sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 1: the nvc crate.

First, investigate rather than assume. Run `nvc --version` and `nvc --help`, and the help for analyse (-a), elaborate (-e) and run (-r). Write what you find to docs/nvc-cli-notes.md: library options (--work, -L), standard selection (--std), waveform output options (--wave and format), and anything else useful. Then deliberately compile a few broken VHDL files from tests/vhdl/ and record exactly what NVC's error and warning output looks like.

Then build:
1. Locating nvc: from a configured path, else PATH. Report a clear error with install hints if missing, and check the version is new enough.
2. A runner that executes analyse, elaborate and run as separate steps, streams stdout/stderr line by line to a callback, can be cancelled, and returns exit status and timing.
3. A diagnostics parser that turns NVC output into structs: severity, file, line, column, message, and any extra context lines. Unparseable lines must be kept as raw text, never dropped.
4. Separate handling for simulation-time output: VHDL report/assert messages with severity and simulation time.
5. Unit tests for the parser using captured real output stored as fixtures in tests/fixtures/.
6. A tiny cli command: `cli run <files...> --top <entity>` that prints parsed diagnostics, so I can test it from a terminal.

Don't add GUI code in this phase.
```

Done when:

- [ ] `cli run` compiles and runs one of your real testbenches end to end
- [ ] Broken files produce clean file:line:message errors, matching what NVC printed
- [ ] Report and assert output shows severity and simulation time
- [ ] Cancelling a long simulation stops the nvc process

## Phase 2: Project and library management

Goal: a project file that replaces the vlib/vmap/vcom scripting you hate, with automatic compile order. Then the first real GUI screen. Expect three or four sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 2: the project crate plus a first GUI screen.

Project file (project.toml at the project root):
- name, VHDL standard (default 2008), output directory for compiled libraries
- libraries, each with a name and a list of source files or glob patterns
- external pre-compiled library paths (for OSVVM, UVVM, vendor libs)
- one or more testbench entries: top entity, generics, stop time, extra nvc options
Propose the exact schema and show me an example before implementing.

Compile order:
- NVC needs files analysed in dependency order. Work out the order automatically by finding package uses, library clauses and entity/component instantiations.
- Evaluate the vhdl_lang crate (from the rust_hdl project, the VHDL language server) for parsing, and check its licence. If it's unsuitable, propose a lightweight scanner instead.
- Detect circular dependencies and report them clearly.
- Incremental builds: only re-analyse files that changed and the files that depend on them. Store hashes in the output directory.

Libraries:
- A function to install OSVVM, UVVM and vendor libraries using `nvc --install`, showing progress.

GUI (app crate):
- Open / create project, recent projects list
- File tree grouped by library
- Testbench dropdown with Build and Run buttons, and a Stop button
- Output panel streaming nvc output, with parsed errors shown as a clickable list
- All nvc work happens on a background thread so the UI never freezes

Test the compile order logic against small multi-file designs in tests/vhdl/.
```

Done when:

- [ ] One of your real multi-file projects builds from `project.toml` without you specifying order
- [ ] Changing one file only recompiles what depends on it
- [ ] OSVVM installs from the GUI and a testbench using it runs
- [ ] The window stays responsive during a long simulation

## Phase 3: Waveform data engine

Goal: load NVC's waveform files and answer "what changed in this time window" fast enough to redraw at 60 fps, even on multi-gigabyte dumps. Kept separate from the UI so it can be tested and optimised on its own. Expect two or three sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 3: the wave crate. No GUI code.

First check docs/nvc-cli-notes.md for which waveform formats NVC writes (FST and VCD) and how to enable them, and update the project crate so runs produce an FST file by default.

Loading:
- Evaluate the wellen crate (used by the Surfer waveform viewer, supports FST and VCD) and check its licence. Use it if suitable rather than writing parsers from scratch. Otherwise propose an alternative.
- Load the hierarchy and signal list first, then load signal data lazily only when a signal is displayed.
- Loading must run in the background with progress, and be cancellable.

Query API (this is what the viewer will call every frame):
- hierarchy: scopes, signals, their VHDL types where available
- for a signal and a time range: the value changes in that range plus the value just before it
- next/previous change after a given time, and next/previous time a signal equals a value
- summarised data for zoomed-out views: when a pixel covers many transitions, return 'many changes here' rather than every change

Values:
- Represent std_logic's nine values (U X 0 1 Z W L H -) properly, not just 0/1
- Formatting helpers: binary, hex, signed, unsigned, and enum names when the type is known

Add benchmarks: generate a large VCD/FST (e.g. 10 million transitions) from a VHDL counter testbench in tests/vhdl/ and measure query time for zoomed-in and fully zoomed-out windows.
```

Done when:

- [ ] Your largest real waveform loads its hierarchy in a couple of seconds
- [ ] Zoomed-out queries over the whole run stay fast in the benchmark
- [ ] U, X and Z values come through correctly, checked against a testbench you know
- [ ] Enum signals (state machines) show their names

## Phase 4: Waveform viewer UI

Goal: the part you'll stare at all day, so it gets the most sessions. Build it in the sub-steps below, one session each, rather than in one prompt. Expect five to eight sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 4: the waveform viewer in the app crate, using the wave crate's query API. We'll build it in steps; today is step [N].

Overall layout: signal tree on the left, signal names + current values column, wave area on the right, time ruler on top, cursor/status bar at the bottom. Draw waves with egui's custom painter, only drawing what's visible.

Step 1 - Basic view: add signals from the tree (double-click and drag-drop), draw single-bit and bus signals, time ruler with sensible units (ps/ns/us/ms), zoom with Ctrl+scroll centred on the mouse, pan with scroll and drag, zoom-to-fit.
Step 2 - std_logic colours: distinct colours for 0/1, U, X, Z (mid-level line), W, and '-', configurable later. Bus values printed inside the bus shape when they fit.
Step 3 - Cursors: primary cursor on click, second cursor with Shift+click, delta time shown, values column follows the primary cursor. Named markers.
Step 4 - Navigation: next/previous edge on the selected signal, jump to next time a signal equals a value, search box to filter the signal tree.
Step 5 - Display options per signal: radix, enum names, rename/alias, colour, grouping into collapsible folders, dividers, reordering by drag.
Step 6 - Layouts and reload: save/restore the wave layout per testbench in the project, and when a simulation re-runs, reload the waveform automatically keeping signals, zoom and cursors.

Performance rule: the UI must stay smooth with a million-transition signal fully zoomed out. Measure frame time and show it in a debug overlay.
```

Done when:

- [ ] Zooming and panning feel instant on your largest real waveform
- [ ] You can debug a real issue in one of your designs using only this viewer
- [ ] Re-running after a code change keeps your whole wave setup
- [ ] Colours and values match what you'd expect from Questa for the same testbench

This is the end of the MVP. Use it on real work for a week or two before starting Phase 5, and keep a list of every annoyance. That list is worth more than this plan for deciding what to build next.

## Phase 5: Source, diagnostics and hierarchy

Goal: connect code, errors and waveforms so you can move between them in one click. Don't build a full code editor; engineers already have one they like. Expect three or four sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 5.

Source viewer:
- Read-only VHDL viewer with syntax highlighting (evaluate syntect via egui_extras, check licences)
- Clicking an error in the diagnostics list opens the file at that line and highlights it
- 'Open in external editor' button and setting, with a configurable command template so it works with VS Code, Vim, Emacs, Notepad++ etc. at the right line
- Watch source files and offer one-click rebuild when they change

Diagnostics panel:
- Separate tabs for build messages and simulation messages (report/assert)
- Filter by severity and search text
- Clicking a simulation message moves the waveform cursor to that simulation time

Design hierarchy:
- Tree of instances from the waveform hierarchy (and from vhdl_lang if we adopted it in Phase 2)
- Selecting an instance lists its ports and signals, with 'add all to waves'
- 'Go to source' for an instance or signal

From a signal in the wave view, 'go to declaration' opens the source viewer at its declaration.
```

Done when:

- [ ] Error to source line, and assert message to waveform time, both work in one click
- [ ] Your usual editor opens at the right line
- [ ] You can add every signal of one instance to the waves in two clicks

## Phase 6: Scripting, compatibility and settings

Goal: one command layer that the GUI, a console, Tcl `.do` scripts and the headless CLI all share. Build the layer first; Tcl is a thin translator on top. Expect three to five sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 6.

Step 1 - Command layer: refactor so every GUI action (build, run, add signal, set radix, move cursor, zoom, save layout) goes through a single command enum with a dispatcher. The GUI sends commands; it never changes state directly. Explain the refactor plan before doing it and keep all existing tests passing.

Step 2 - Console: a console panel where I can type commands, with history and tab-completion of signal paths. Every GUI action echoes its equivalent command, so I learn the syntax by using the GUI.

Step 3 - Tcl compatibility: embed a Tcl interpreter (compare the available Rust options for embedding the real Tcl C library, check licences and Windows build pain) and register ModelSim-style commands that map onto our command layer. Start with: vlib, vmap, vcom, vsim, run, restart, add wave, force, noforce, examine, quit, and do. Write a table in docs/tcl-compat.md listing each command, which flags we support, and differences from ModelSim.

Step 4 - Headless CLI: `cli build`, `cli test` (runs all testbenches, exits non-zero on failure, prints a summary, optional JUnit XML for CI) and `cli do script.do`.

Step 5 - Settings: themes (light/dark), wave colours, keyboard shortcuts, nvc path, editor command. Stored in a per-user config file.
```

I'll give Claude the commands in Step 3 as a starting list; you'll know better than anyone which ones your old `.do` files actually use, so edit the list from a few of your real scripts before running that step.

Done when:

- [ ] A simplified version of one of your real `.do` files runs unchanged
- [ ] `cli test` runs all testbenches and works in a CI job
- [ ] Every common GUI action has a console equivalent

## Phase 7: Packaging and performance

Goal: download, install, simulate in minutes, the experience you said no current tool delivers. Expect two or three sessions.

```text
Read CLAUDE.md and docs/PLAN.md. We're doing Phase 7.

Packaging:
- Installers for Windows (MSI or similar), Linux (AppImage and .deb) and macOS (.dmg), built in GitHub Actions on each tagged release
- First-run wizard: find nvc or offer download instructions, check the version, offer to install OSVVM/UVVM, then open an example project
- Include a small example project (counter + testbench, plus a state machine) that runs on first launch
- If we bundle the nvc binary, include its GPL licence text and source link, and keep it as a separate executable

Performance pass:
- Profile startup time, project open, build of a large project, and waveform load; write results to docs/performance.md
- Fix the top three bottlenecks you find and show me before/after numbers

Robustness:
- Crash reporting to a local log file, autosave of layouts, graceful handling of nvc crashing or being killed mid-simulation
```

Done when:

- [ ] A colleague installs it on a clean machine and runs the example in under 5 minutes, without your help
- [ ] Killing nvc mid-run leaves the GUI working
- [ ] Startup to usable window is under 2 seconds

## Later: interactive debugging

Breakpoints, stepping and live waveforms need a connection into the running simulation, which is harder and is where the GPL grey area lives. Leave it until the MVP is in daily use.

The route to investigate is VHPI, the IEEE standard programming interface that NVC supports (it's how cocotb drives NVC). A small VHPI plugin loaded by NVC would report value changes and accept commands, talking to the GUI over a local socket. Because that plugin runs inside the NVC process, take legal advice on its licensing before including it in anything you sell; one option is to release just the plugin as open source.

```text
Read CLAUDE.md and docs/PLAN.md. Research only, no code yet: investigate how to add interactive debugging to our GUI.

1. Check what NVC currently offers itself: run nvc --help, read its docs for any interactive, Tcl or debugging modes, and check which VHPI features it supports.
2. Propose a design for a VHPI plugin that streams value changes and supports pause, resume, run-for-time and simple breakpoints (signal equals value, time reached, assert fired), communicating with the GUI over a local socket.
3. List the risks and the parts of VHPI that would be hardest.
4. Note the licensing question: the plugin runs inside the GPL nvc process. Recommend how to structure it so the GUI stays independent.

Write the findings to docs/debugging-design.md.
```

Code coverage display is a gentler second candidate: NVC can collect coverage, so the GUI would only need to read and display its reports. Ask Claude to check NVC's coverage options and output format first.

## Testing strategy and troubleshooting prompts

Your VHDL test designs are the most valuable thing in the repo. Grow `tests/vhdl/` every time you find a bug, so it can never come back.

What to keep in it:

- Small designs that exercise each display case: std\_logic U/X/Z, wide buses, signed values, enums, records, arrays
- Deliberately broken files for every kind of error message you've seen
- One large generated testbench for performance (millions of transitions)
- Two or three of your own real projects, if you can share them with yourself, as end-to-end checks

Prompts for when things go wrong:

```text
This is broken: [describe what you see vs what you expected]. Don't fix anything yet. Reproduce it with a test first, explain the root cause in plain terms, then propose a fix.
```

```text
We've made several attempts at this and it's getting messier. Stop. Summarise what we've tried, what we learned, and propose a cleaner approach. I'll decide whether to git reset and start again.
```

```text
Explain how [module/feature] works as if to an experienced hardware engineer who isn't a software engineer. Use an analogy to hardware where it helps, then show me the three most important functions.
```

```text
Review the whole codebase for problems that will hurt us later: duplicated logic, modules doing too much, missing tests, slow paths in the waveform rendering. Rank them and suggest which to fix now.
```

Run that last review prompt at the end of each phase, in a fresh session, so Claude looks at the code without the bias of having just written it.
