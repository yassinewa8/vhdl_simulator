# NVC Studio (placeholder name)

A desktop GUI and waveform viewer for the [NVC](https://github.com/nickg/nvc)
VHDL simulator. NVC is run as a separate program; this project does not
include or link any NVC code.

Status: Phase 0 (empty window). See [docs/PLAN.md](docs/PLAN.md).

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
- **NVC** on your PATH (needed from Phase 1 on): see the
  [NVC install guide](https://github.com/nickg/nvc#installing).

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
