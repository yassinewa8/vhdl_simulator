# VHDL test designs

Small VHDL designs used by the tests. Add a new one every time a bug is found,
so it can't come back.

What belongs here:

- Designs that exercise each display case: std_logic U/X/Z, wide buses,
  signed values, enums, records, arrays
- Deliberately broken files, one per kind of NVC error message
- Multi-file designs for testing compile order
- One large generated testbench for performance (millions of transitions)

Start with `counter.vhd`, a counter with a self-stopping testbench.
