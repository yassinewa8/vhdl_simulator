-- Two drivers on one std_logic signal, stepping through every pair of the
-- nine std_logic values (81 steps, 10 ns each, in the order U X 0 1 Z W L H -
-- for driver_a, then the same order for driver_b within each).
--
-- The waveform test checks `resolved` against the resolution table in the
-- IEEE std_logic_1164 standard, so the expected values come from the
-- standard rather than from this design. It also covers U before anything
-- is driven, Z when both drivers let go, and X on contention.
library ieee;
use ieee.std_logic_1164.all;

entity resolution_tb is
end entity resolution_tb;

architecture sim of resolution_tb is
    type logic_list is array (natural range <>) of std_ulogic;
    constant ALL_VALUES : logic_list := ('U', 'X', '0', '1', 'Z', 'W', 'L', 'H', '-');

    signal driver_a : std_ulogic := 'U';
    signal driver_b : std_ulogic := 'U';
    signal resolved : std_logic;
begin
    -- Two concurrent drivers: std_logic's resolution function decides.
    resolved <= driver_a;
    resolved <= driver_b;

    stimulus : process
    begin
        for a in ALL_VALUES'range loop
            for b in ALL_VALUES'range loop
                driver_a <= ALL_VALUES(a);
                driver_b <= ALL_VALUES(b);
                wait for 10 ns;
            end loop;
        end loop;
        wait;
    end process;
end architecture sim;
