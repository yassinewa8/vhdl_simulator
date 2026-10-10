# NVC command-line notes

Findings from running NVC **1.23.0** on Windows 11 (2026-10-09). Real captured
output lives in `tests/fixtures/nvc/`; regenerate it with
`bash scripts/capture-nvc-fixtures.sh` after upgrading NVC.

## Command structure

```
nvc [GLOBAL OPTIONS] COMMAND [COMMAND OPTIONS] [COMMAND [COMMAND OPTIONS]]...
```

- **Global options go before the command**: `--std`, `--work`, `-L`, `--map`,
  `--messages`, `--stderr`, `--ieee-warnings`, `-H`, `-M`, `--seed`.
- Commands: `-a FILE...` (analyse), `-e TOP` (elaborate), `-r TOP` (run).
  Several can be chained in one call (`-a f.vhd -e tb -r tb`), but we run
  them as separate processes so each step has its own exit code and timing.
- Other commands of interest later: `--install PKG` (OSVVM/UVVM, Phase 2),
  `--list` (units in a library), `--print-deps` (Makefile-style dependencies,
  possibly useful for compile order in Phase 2), `--do`/`-i` (Tcl, Phase 6),
  `--cover-*` (coverage).

## Libraries

- `--work=NAME` or `--work=NAME:PATH`. Default is library `work` in `./work`.
- **NVC does not create parent folders.** `--work=lib:out/lib` fails with
  `** Fatal: mkdir: out/lib: The system cannot find the path specified.`
  if `out/` doesn't exist. We must create the parent folder first.
- Library folder contents: `_NVC_LIB`, `_index`, and one file per unit
  (`LIB.ENTITY`, `LIB.ENTITY-ARCH`, `LIB.TOP.elab`).
- `-L PATH` adds a search path; `--map=LIB:PATH` maps a single library.
  Built-in search path: `C:\Program Files\NVC\lib\nvc` (IEEE, STD etc.).
- `--map` to a folder that doesn't exist yet prints
  `** Warning: library b not found at out/b` (but carries on). `-L` to a
  missing folder is silent. So the build only maps project libraries
  whose folder already exists.
- `--std` must match between analysis and elaboration, otherwise:
  `** Fatal: ...: design unit was analysed using standard revision 2008 which
  is more recent that the currently selected standard 1993`.

## Installing libraries (`--install`)

Checked 2026-10-10 with OSVVM 2025.06a on Windows 11.

- `nvc --install PKG` runs `bash <prefix>/libexec/nvc/install-PKG.sh`,
  finding `bash` on PATH. The package list is the `install-*.sh` file
  names (fmf, icecube2, ise, libero, osvvm, quartus, uvvm, vivado, vunit,
  xpm_vhdl); an unknown name prints that list.
- **Bug on Windows (1.23.0): the script path isn't quoted.** With nvc in
  `C:\Program Files\NVC`, bash gets `C:\Program` and fails with status
  127. Without bash on PATH it fails with status -1. So we run the script
  ourselves with bash (`nvc::install_package`), passing the path with `/`
  separators (the scripts use `dirname "$BASH_SOURCE"`, which doesn't
  understand `\`) and `NVC=<path to nvc>` so they use the same nvc.
- Needs bash and git; OSVVM also needs `tclsh` with Tcllib's `fileutil`.
  Git for Windows has all but Tcllib, but only through its launcher
  `Git\bin\bash.exe` (sets `MSYSTEM=UCRT64` and PATH); `Git\usr\bin\bash.exe`
  sees no git or tclsh. Tcllib can be supplied with `TCLLIBPATH`.
- Sources are cloned to `~/.cache/nvc/<repo>-<tag>` and reused next time.
  Libraries are compiled into `~/.nvc/lib` (`NVC_INSTALL_DEST` changes it).
- OSVVM is compiled for 2008 and 2019 into folders like `OSVVM.08`,
  `OSVVM.19`; UVVM likewise (`uvvm_util.08`). Install takes ~15 s.
- nvc searches `~/.nvc/lib` by default, and picks the `.08`/`.19` folder
  matching `--std` (a `.08`-only library is "not found" with
  `--std=2019`). **But it finds `~` through `HOME` only**, which Windows
  programs normally don't have (Git Bash and MSYS2 set it). Without it:
  `** Error: library OSVVM not found`. So we always pass
  `-L <USERPROFILE>/.nvc/lib`; the `.08`/`.19` choice works with `-L` too.
- OSVVM's `EndOfTestReports` fails outside OSVVM's own Tcl flow
  (`failed to open OsvvmTemp_NVC/OsvvmRun.yml`); use `ReportAlerts`.
  OSVVM's report lines go to **stdout**.

## Useful options

| Step | Option | Meaning |
|------|--------|---------|
| global | `--std={1993,2000,2002,2008,2019}` | VHDL revision |
| global | `--messages={full,compact}` | Message style (see below) |
| global | `--stderr=SEVERITY` | Minimum severity sent to stderr |
| analyse | `--relaxed` | Downgrade some errors to warnings |
| analyse | `--check-synthesis` | Extra warnings (e.g. sensitivity lists) |
| analyse | `-Werror` | Warnings become errors |
| analyse | `--error-limit=N` | Stop after N errors |
| analyse | `-f LIST` | Read file list from a file |
| elaborate | `-gNAME=VALUE` | Set a top-level generic |
| elaborate | `-O0..-O3`, `-j` | Optimisation level, JIT |
| run | `--stop-time=T` | Stop at simulation time T (e.g. `5ns`) |
| run | `--exit-severity=SEV` | Stop on first report/assert of SEV or worse |
| run | `-w[FILE]`, `--wave[=FILE]` | Write a waveform; default name `TOP.fst` in the current folder |
| run | `--format={fst,vcd}` | Waveform format (default FST) |
| run | `--include/--exclude=GLOB` | Filter dumped signals |
| run | `--dump-arrays[=N]` | Include nested arrays in the dump |
| run | `--stats` | Print time/memory at the end (as a Note) |

## Exit codes

| Situation | Exit code |
|-----------|-----------|
| Success, including warnings only | 0 |
| Analysis or elaboration error | 1 |
| Simulation reported any `error` or `failure` (even if it ran to the end) | 1 |
| `--stop-time` reached | 0 |
| Process killed | -1 / non-zero |

## Output streams

- **All diagnostics and report/assert messages go to stderr**, both styles.
  stdout was empty in every case tested (only `--list`/`--version` use it).
- Output **streams line by line** through a pipe (no buffering problem): a
  testbench reporting every ~35 ms produced lines ~35 ms apart.
- Lines end in `\r\n` on Windows. The parser must accept `\r\n` and `\n`.
- No colour codes when stderr is a pipe.
- File paths are printed as given on the command line, with forward slashes
  (`C:/claude/...` for absolute paths on Windows).

## Message format: full (default)

Analysis/elaboration diagnostic:

```
** Error: no visible declaration for B
    > tests/vhdl/broken/undeclared.vhd:11
    |
 11 |     a <= b;
    |          ^ did you mean A?
```

- Header: `** <Severity>: <message>`. Severities seen: `Note`, `Warning`,
  `Error`, `Failure`, `Fatal`.
- Location: `> FILE:LINE` (no column). Missing for some fatals
  (`** Fatal: cannot find unit WORK.NO_SUCH_TB in library WORK`).
- Then a source snippet with `^^^` markers and optional labels
  ("did you mean A?", "missing from sensitivity list", "read here").
  A snippet can show several source lines (see `analyse_warnings.full.txt`).
- Optional trailing notes: `   = Note: the --relaxed option downgrades this to a warning`.
- No column number: it would have to be worked out from the `^` position.

Simulation message:

```
** Warning: 10ns+0: a warning message
   Process :messages_tb:_p0 at tests/vhdl/sim/messages_tb.vhd:8
```

- The location given is the **line where the process starts**, not the
  `report`/`assert` statement.
- `assert false` without a message prints `Assertion violation.` and a
  source snippet instead of the `Process ... at` line.

## Message format: compact (`--messages=compact`)

```
tests/vhdl/broken/undeclared.vhd:11:10: error: no visible declaration for B
tests/vhdl/sim/messages_tb.vhd:12:9: warning: 10ns+0: a warning message
fatal: cannot find unit WORK.NO_SUCH_TB in library WORK
```

- One line per message: `FILE:LINE:COL: severity: message`, or just
  `severity: message` when there's no location.
- Severities in lower case: `note`, `warning`, `error`, `failure`, `fatal`.
- Has the **column**, and for simulation messages points at the actual
  `report`/`assert` statement (line 12 above vs. line 8 in full style).
- **Loses** the hint labels ("did you mean A?") and `= Note:` lines.
- Windows paths contain a drive colon (`C:/...`), so parse from the
  `:LINE:COL: severity:` part rather than splitting on the first colon.

## Simulation time format

`<number><unit>+<delta>`, e.g. `0ms+0`, `10ns+0`, `15001ps+0`, `3015001ps+0`.
NVC picks the largest unit that represents the time exactly (15.001 ns is
printed as `15001ps`). Units seen so far: `ps`, `ns`, `ms`. The parser
should also accept `fs`, `us`, `sec`, `min` and `hr` (the VHDL `time`
units), which haven't been observed yet. Time zero is printed as `0ms`.

## Cancelling

- `nvc -r` is a single process (no child processes). Killing it stops the
  simulation immediately; exit code -1 when force-killed on Windows.
