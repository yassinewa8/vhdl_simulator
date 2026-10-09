# NVC Studio (placeholder name)

A desktop GUI and waveform viewer for the [NVC](https://github.com/nickg/nvc)
VHDL simulator. NVC is run as a separate program; this project does not
include or link any NVC code.

Status: Phase 1 (nvc integration + `cli run`). See [docs/PLAN.md](docs/PLAN.md).

## Try it

```bash
cargo run -p cli -- run tests/vhdl/counter.vhd --top counter_tb
```

prints

```
Using nvc 1.23.0 (C:\Program Files\NVC\bin\nvc.exe)
-- analyse: ok (0.04 s)
-- elaborate: ok (0.05 s)
[120 ns] note: count = 10  (at tests/vhdl/counter.vhd:56:9)
-- run: ok (0.05 s)
PASSED
```

Options: `--std 2008`, `--stop-time 10us`, `-g NAME=VALUE`, `--work DIR`,
`--wave FILE.fst`, `--nvc PATH`. See `cargo run -p cli -- run --help`.
The exit code is 0 if everything passed, 1 otherwise.

## Building

### Prerequisites

- **Rust** via [rustup](https://rustup.rs). The exact version is pinned in
  `rust-toolchain.toml` and installed automatically on first build.
- **Windows:** Visual Studio Build Tools with the "Desktop development with
  C++" workload (the rustup installer offers this).
- **Linux (Debian/Ubuntu):**
  ```bash
  sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libgl1-mesa-dev
  ```
- **NVC 1.23.0 or newer** on your PATH: see the
  [NVC releases](https://github.com/nickg/nvc/releases). Without it, the
  end-to-end tests are skipped (with a message) and everything else works.

### Commands

```bash
cargo build --workspace          # build everything
cargo run -p app                 # open the GUI
cargo run -p cli                 # run the command-line tool
cargo test --workspace           # run all tests
cargo clippy --workspace --all-targets -- -D warnings   # lint
```

## Layout

| Folder           | What it does |
|------------------|--------------|
| `crates/core`    | Shared types (package name `studio-core`) |
| `crates/nvc`     | Runs `nvc` and parses its output |
| `crates/project` | Project files and build orchestration |
| `crates/wave`    | Waveform loading and querying |
| `crates/app`     | Desktop GUI (the only crate with GUI code) |
| `crates/cli`     | Headless command-line tool |
| `tests/vhdl`     | Small VHDL test designs |
| `docs`           | Plan, decisions and notes |
