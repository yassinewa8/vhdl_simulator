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

# Waveforms, read by the wave crate's tests. Each design is dumped as FST
# (what runs write by default) and VCD. The VCD's $date block is removed so
# re-capturing only shows a diff when the content changes. (The FST is
# binary and records the date too, so it always differs; compare the VCD.)
#
# nvc writes the source file's absolute path into the waveform, unquoted.
# In a VCD, a space in that path makes the file unreadable (see
# docs/nvc-cli-notes.md), and this repo may live in a folder with a space.
# So the designs are copied to a fixed temp folder without spaces first,
# and one VCD is deliberately made from a folder WITH a space, to test the
# error message for that case.
waves="tests/fixtures/waves"
mkdir -p "$waves"
tmp="${TMPDIR:-/tmp}"
clean_dir="$tmp/nvc-studio-fixtures"
spaced_dir="$tmp/nvc studio fixtures"
case "$clean_dir" in *" "*) echo "error: temp folder '$clean_dir' has a space"; exit 1;; esac
rm -rf "$clean_dir" "$spaced_dir"
mkdir -p "$clean_dir" "$spaced_dir"
cp tests/vhdl/waves/*.vhd "$clean_dir/"
cp tests/vhdl/waves/types_tb.vhd "$spaced_dir/"

for tb in resolution_tb types_tb; do
    nvc --std=2008 --work="work:$work/work" -a "$clean_dir/$tb.vhd" -e "$tb"
    nvc --std=2008 --work="work:$work/work" -r --wave="$waves/$tb.fst" "$tb"
    echo "$tb fst exit=$?"
    nvc --std=2008 --work="work:$work/work" -r --format=vcd --wave="$waves/$tb.vcd" "$tb"
    echo "$tb vcd exit=$?"
    sed -i '/^\$date/,/^\$end/d' "$waves/$tb.vcd"
done

nvc --std=2008 --work="work:$work/work" -a "$spaced_dir/types_tb.vhd" -e types_tb
nvc --std=2008 --work="work:$work/work" -r --format=vcd --wave="$waves/space_in_source_path.vcd" types_tb
echo "space_in_source_path vcd exit=$?"
sed -i '/^\$date/,/^\$end/d' "$waves/space_in_source_path.vcd"
rm -rf "$clean_dir" "$spaced_dir"
