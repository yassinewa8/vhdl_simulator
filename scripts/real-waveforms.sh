#!/usr/bin/env bash
# Produces realistic waveforms from professional verification code, as a
# stand-in for "your largest real waveform" when testing the wave crate
# and the viewer:
#
#   UVVM:  Bitvis UART demo (uart_vvc_demo_tb), built and run through
#          OUR project system (cli build --run), so it is also a
#          real-world test of automatic compile order.
#   OSVVM: the AXI4 demo tests, run through OSVVM's own scripts with
#          SetSaveWaves.
#
# Needs: nvc, and UVVM and OSVVM installed with `cli install uvvm` /
# `cli install osvvm` (their sources are reused from ~/.cache/nvc).
# OSVVM's scripts also need tclsh with Tcllib (see docs/nvc-cli-notes.md).
#
# Output goes to target/real-waves/. Time a result with:
#   cargo run --release -p wave --example bench -- --file <file.fst>

set -eu
cd "$(dirname "$0")/.."

# Native (Windows) path form when running under Git Bash/MSYS2, since nvc
# and tclsh there are Windows programs.
native() { if command -v cygpath > /dev/null; then cygpath -m "$1"; else echo "$1"; fi; }

cache="$(native "$HOME")/.cache/nvc"
uvvm="$(ls -d "$cache"/UVVM-* 2> /dev/null | tail -1)"
osvvm="$(ls -d "$cache"/OsvvmLibraries-* 2> /dev/null | tail -1)"
out="$(native "$PWD")/target/real-waves"
mkdir -p "$out"

if [ -n "$uvvm" ]; then
    echo "== UVVM UART demo ($uvvm)"
    project="$out/uvvm_uart"
    rm -rf "$project"
    mkdir -p "$project"
    cat > "$project/project.toml" << TOML
[project]
name = "uvvm_uart_demo"
analyse_args = ["--relaxed"]   # as UVVM's own scripts use

# The UART design and its demo test harness, compiled into bitvis_uart as
# in UVVM's scripts. The UVVM libraries it uses come from ~/.nvc/lib.
[[library]]
name = "bitvis_uart"
files = ["$uvvm/bitvis_uart/src/*.vhd", "$uvvm/bitvis_uart/tb/uart_vvc_demo_th.vhd", "$uvvm/bitvis_uart/tb/uart_vvc_demo_tb.vhd"]

[[testbench]]
top = "uart_vvc_demo_tb"
TOML
    cargo run -q --release -p cli -- build "$project" --run uart_vvc_demo_tb | tail -5
else
    echo "UVVM not found in $cache: run 'cli install uvvm' first"
fi

if [ -n "$osvvm" ]; then
    echo "== OSVVM AXI4 demo tests ($osvvm)"
    # OSVVM's scripts pass --wave=<path> to nvc unquoted, so they fail in a
    # folder whose path has a space (as this repo's may). Run them in a
    # space-free temp folder and copy the waveforms back.
    run="$(native "${TMPDIR:-/tmp}")/nvc-studio-osvvm-axi4"
    case "$run" in *" "*) echo "error: temp folder '$run' has a space"; exit 1;; esac
    rm -rf "$run"
    mkdir -p "$run"
    cat > "$run/run.tcl" << TCL
source $osvvm/Scripts/StartNVC.tcl
SetSaveWaves
build $osvvm/OsvvmLibraries.pro
build $osvvm/AXI4/Axi4/RunDemoTests.pro
TCL
    tcllib="$(native "$HOME")/.cache/nvc-studio/tcllib/modules"
    if [ -d "$tcllib" ]; then export TCLLIBPATH="$tcllib"; fi
    (cd "$run" && tclsh run.tcl > run.log 2>&1) || echo "OSVVM run failed: see $run/run.log"
    grep "^Build" "$run/run.log" | tail -1
    rm -rf "$out/osvvm_axi4"
    mkdir -p "$out/osvvm_axi4"
    find "$run" -name "*.fst" -exec cp {} "$out/osvvm_axi4/" \;
    cp "$run/run.log" "$out/osvvm_axi4/"
    ls "$out/osvvm_axi4"/*.fst
else
    echo "OSVVM not found in $cache: run 'cli install osvvm' first"
fi
