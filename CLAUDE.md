# CLAUDE.md

A desktop GUI and waveform viewer for the NVC VHDL simulator, written in Rust
(eframe/egui). Read `docs/PLAN.md` and `docs/ARCHITECTURE.md` (code map,
testing, known gotchas) at the start of every session.

## About the user

An FPGA engineer who knows VHDL and simulators deeply but is not an
experienced software engineer. Explain architectural decisions in plain
terms (hardware analogies help) and ask before making big ones. Record
agreed decisions in the "Decisions" section of `docs/PLAN.md`.

## Rules

1. **NVC is GPLv3. Only ever run it as a subprocess.** Never link libnvc and
   never copy NVC source into this repo. Exchange data only through
   command-line arguments, stdout/stderr, and files NVC writes (waveforms,
   compiled libraries).
2. **Check the licence of every new crate dependency** before adding it. Tell
   the user before adding anything that isn't MIT, Apache-2.0, BSD or
   MPL-2.0 (flag GPL/LGPL/AGPL/EUPL especially). Note the licence next to the
   dependency in the root `Cargo.toml`.
3. **GUI code lives only in `crates/app`.** All other crates must be testable
   without a GUI and must not depend on egui/eframe. Keep logic out of the
   app crate where possible so it can be tested.
4. **Every feature gets tests.** Before saying a task is done, run all three
   and make sure they pass with no warnings:
   ```
   cargo build --workspace
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   ```
5. **Prefer simple, readable code over clever code.** The user needs to be
   able to follow it. Short functions, descriptive names, comments that
   explain *why*.

## Workspace layout

| Crate (folder)     | Package name  | Purpose |
|--------------------|---------------|---------|
| `crates/core`      | `studio-core` | Shared types: diagnostics, time values, signal values. Named `studio-core` because `core` clashes with Rust's built-in library. |
| `crates/nvc`       | `nvc`         | Runs `nvc` as a subprocess and parses its output. |
| `crates/project`   | `project`     | Project file loading/saving and build orchestration. |
| `crates/wave`      | `wave`        | Waveform loading and querying. No GUI code. |
| `crates/app`       | `app`         | Desktop GUI (eframe/egui). |
| `crates/cli`       | `cli`         | Headless command-line tool reusing `project` + `nvc`. |

Shared dependency versions go in `[workspace.dependencies]` in the root
`Cargo.toml`; crates refer to them with `name.workspace = true`.

Small VHDL test designs live in `tests/vhdl/`. Real NVC output used by the
parser tests lives in `tests/fixtures/nvc/`; regenerate it with
`bash scripts/capture-nvc-fixtures.sh` (never hand-edit fixtures). What NVC's
command line and output look like is recorded in `docs/nvc-cli-notes.md`.

## Notes

- If the `nvc` binary is ever bundled in an installer, ship its licence text
  and a link to its source alongside it, as a separate executable.
- Interactive debugging via a plugin loaded into the NVC process (VHPI) is a
  licensing grey area. Don't build it without the user's explicit go-ahead.
