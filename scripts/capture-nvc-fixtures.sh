#!/usr/bin/env bash
# Captures real NVC output for the designs in tests/vhdl/ and saves it to
# tests/fixtures/nvc/. The nvc crate's parser tests read these files.
#
# Re-run this after upgrading NVC to check the output format hasn't changed:
#     bash scripts/capture-nvc-fixtures.sh
#     git diff tests/fixtures
#
# Each fixture is saved twice: full message style (*.full.txt) and
# --messages=compact style (*.compact.txt). Only stderr is captured; NVC
# writes nothing to stdout for these cases.

set -u
cd "$(dirname "$0")/.."

out="tests/fixtures/nvc"
work="target/fixture-work"
mkdir -p "$out"
rm -rf "$work"
mkdir -p "$work"

nvc --version | head -1 > "$out/nvc-version.txt"

# capture NAME ARGS...  -> runs nvc in both message styles
capture() {
    local name="$1"
    shift
    nvc --std=2008 --work="work:$work/work" "$@" 2> "$out/$name.full.txt"
    echo "$name full exit=$?"
    nvc --std=2008 --work="work:$work/work" --messages=compact "$@" 2> "$out/$name.compact.txt"
    echo "$name compact exit=$?"
}

# Analysis errors and warnings
for f in syntax_error undeclared type_mismatch multiple_errors missing_unit; do
    capture "analyse_$f" -a "tests/vhdl/broken/$f.vhd"
done
capture analyse_warnings -a --relaxed --check-synthesis tests/vhdl/broken/warnings.vhd

# Elaboration errors (need the counter analysed first)
nvc --std=2008 --work="work:$work/work" -a tests/vhdl/counter.vhd tests/vhdl/sim/messages_tb.vhd
capture elab_missing_top -e no_such_tb
capture elab_bad_generic -e -gWIDTH=0 counter
capture elab_unused_generic -e -gNOPE=3 counter

# Simulation-time report/assert output
nvc --std=2008 --work="work:$work/work" -e messages_tb
capture run_messages -r messages_tb
capture run_counter -e counter_tb -r counter_tb
