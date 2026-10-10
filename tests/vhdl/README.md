# VHDL test designs

Small VHDL designs used by the tests. Add a new one every time a bug is found,
so it can't come back.

What belongs here:

- Designs that exercise each display case: std_logic U/X/Z, wide buses,
  signed values, enums, records, arrays
- Deliberately broken files, one per kind of NVC error message
- Multi-file designs for testing compile order
- One large generated testbench for performance (millions of transitions)

Current contents:

- `counter.vhd`: a counter with a self-stopping testbench (`counter_tb`)
- `broken/`: files that fail analysis (or warn, in `warnings.vhd`), one
  kind of error each
- `sim/messages_tb.vhd`: a report at every severity, then failed asserts
- `sim/endless_tb.vhd`: never stops by itself; used to test Stop/cancel
- `waves/resolution_tb.vhd`: two drivers through all 81 std_logic pairs;
  the wave crate checks the result against the IEEE 1164 resolution table
- `waves/types_tb.vhd`: one signal of each type the viewer must show
- `waves/clock_tb.vhd`: 5 ns clock and 8-bit counter; 1 ms gives 200,001
  clock changes for testing queries on a busy signal
- `projects/three_libs/`: a three-library project (`project.toml`) whose
  file names sort in the wrong order, for testing automatic compile order,
  incremental rebuilds and running a testbench from a project
- `projects/osvvm_tb/`: a parity block checked with OSVVM (random data,
  `AffirmIfEqual`); needs OSVVM installed (`cli install osvvm`)

After adding a design whose NVC output a parser test should check, add it
to `scripts/capture-nvc-fixtures.sh` and re-run the script.
