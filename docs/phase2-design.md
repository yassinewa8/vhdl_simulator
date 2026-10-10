# Phase 2 design proposal

Status: **accepted 2026-10-10.** All four recommendations were approved
(scanner, schema as proposed, test `nvc --install osvvm`, add `cli build`);
recorded in `docs/PLAN.md` (Decisions).

## Findings

- `nvc --print-deps` only works on files that are **already analysed**, so it
  can't give the compile order for a first build. We must work it out from
  the source files ourselves.
- `vhdl_lang` 0.88.0 (rust_hdl, the VHDL language server) is **MPL-2.0**;
  its ~70 dependencies are all permissive (MIT/Apache/BSD/Unicode-3.0/CC0/
  Unlicense). Usable, but:
  - finding instantiations means walking the whole syntax tree (statements,
    generates, blocks);
  - on some syntax errors it returns nothing for the whole file;
  - it's 0.x, so the API changes often.
- `nvc --install PKG` packages: fmf, icecube2, ise, libero, osvvm, quartus,
  uvvm, vivado, vunit, xpm_vhdl.
  - They are **bash scripts** (`C:\Program Files\NVC\libexec\nvc\install-*.sh`).
    On Windows they need bash, git and (for OSVVM) tclsh. This machine has
    them via MSYS2 (`/usr/bin/bash`, `/ucrt64/bin/git`, `/ucrt64/bin/tclsh`).
    Not yet tested whether this works when the GUI runs the install.
    *Correction (step 3): these come from Git for Windows, not MSYS2, and
    `nvc --install` itself is broken on Windows; see
    `docs/nvc-cli-notes.md` and the Decisions in `docs/PLAN.md`.*
  - Sources are cloned to `~/.cache/nvc/<name>-<tag>` (OSVVM: OsvvmLibraries
    tag 2025.06a), compiled into `~/.nvc/lib` by default; `--dest=DIR`
    changes that.
  - Not yet verified whether `~/.nvc/lib` is on nvc's default library search
    path (`nvc --help` only lists `C:\Program Files\NVC\lib\nvc`).

## Proposal

### Compile order: our own small scanner (recommended over vhdl_lang)

Strip comments and strings, then look for these patterns:

| Provides | Depends on |
|---|---|
| `entity X is`, `package X is`, `context X is`, `configuration C of E` | `use lib.pkg.…`, `context lib.ctx` |
| `architecture A of E` | entity E |
| `package body X` | package X |
| | `entity lib.X` and `u1 : comp port map` instantiations, `package P is new lib.G` |

It still finds units in files that have errors (so NVC gets to report the
real error), is ~300 readable lines, and adds no dependency. Names that
don't match a unit in the project (e.g. vendor primitives) are ignored.
vhdl_lang can still be adopted in Phase 5 for go-to-declaration.

### project.toml schema

```toml
[project]
name = "uart"
standard = "2008"            # optional, default 2008
output_dir = "build"         # optional: compiled libraries, build state, waves
analyse_args = ["--relaxed"] # optional extra nvc options for every analysis

# Your libraries, compiled into build/<name>/. Order in this file doesn't matter.
[[library]]
name = "uart_lib"
files = ["src/**/*.vhd"]     # paths or glob patterns, relative to project.toml

[[library]]
name = "tb_lib"
files = ["tb/*.vhd", "models/uart_model.vhd"]

# Pre-compiled libraries (OSVVM, UVVM, vendor libs), all optional.
[external]
search_paths = ["C:/vendor/nvc-libs"]           # nvc -L
map = { unisim = "C:/xilinx/nvc/unisim" }       # nvc --map=LIB:PATH
# Libraries from "Install OSVVM/UVVM" (~/.nvc/lib) are added automatically.

[[testbench]]
top = "uart_tx_tb"           # entity name; its library is found automatically
generics = { BAUD = "115200", DATA_BITS = "8" }
stop_time = "10 ms"
elaborate_args = ["--jit"]
run_args = ["--exit-severity=error"]

[[testbench]]
top = "uart_rx_tb"
name = "rx, fast baud"       # optional display name, default = top
generics = { BAUD = "921600" }
```

### Build

- Scan every file. A file is compiled after every file that defines a unit
  it uses, across libraries. Ties keep the order the files are listed in.
- Circular dependencies are reported as a chain, e.g.
  `a.vhd (pkg_a) → b.vhd (pkg_b) → a.vhd`.
- Incremental: `build/build-state.toml` stores a content fingerprint of each
  file (contents, not dates). A changed file is recompiled together with
  everything that depends on it, directly or indirectly. Changing the
  standard or `analyse_args` rebuilds everything.
- Project libraries are passed to nvc with `--map=LIB:build/LIB`.
- Running a testbench elaborates and runs it via the existing `nvc` crate.

### First GUI screen (app crate)

- File menu: New project, Open project, Recent projects.
- Left: files grouped by library.
- Toolbar: testbench dropdown, Build, Run, Stop.
- Bottom: live output log, plus a Problems list of parsed errors. Clicking a
  problem selects the file and shows NVC's full message with its snippet.
  Jumping to the line in an editor is Phase 5.
- All nvc work runs on a background thread, which sends events to the UI
  through a channel, so the window never freezes.
- Install OSVVM/UVVM menu items, showing live install output as progress.

### New dependencies (all within the licence rules)

`serde` (MIT/Apache), `toml` (MIT/Apache), `glob` (MIT/Apache), `rfd` (MIT,
native folder dialogs), eframe's `persistence` feature (`ron`, MIT/Apache)
for recent projects.

### Steps (commit after each)

1. Project file loading/saving and the scanner.
2. Compile order and incremental builds, plus `cli build`.
3. Library install (`nvc --install`).
4. GUI.

## Open questions for the user

1. Scanner instead of vhdl_lang for compile order? (Recommended.)
2. Is the schema right? Anything to rename, add or drop (e.g. per-file
   standard, per-library options)?
3. OK to run `nvc --install osvvm` for testing? It downloads OSVVM from
   GitHub (tens of MB) into `~/.cache/nvc` and compiles into `~/.nvc/lib`.
4. Add `cli build` now, to try compile order on real projects before the GUI
   exists? (Recommended; Phase 6 would then only add `cli test`.)
