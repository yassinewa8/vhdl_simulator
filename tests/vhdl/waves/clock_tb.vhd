-- A 5 ns half-period clock and an 8-bit counter that increments on each
-- rising edge. Run for 1 ms it gives 200,001 clock changes, used to test
-- the waveform queries on a busy signal with answers that can be worked
-- out by hand: the clock rises at 5, 15, 25 ns..., so the counter becomes
-- N at (10 * N - 5) ns, wrapping every 256 counts.
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity clock_tb is
end entity clock_tb;

architecture sim of clock_tb is
    signal clk   : std_logic := '0';
    signal count : unsigned(7 downto 0) := (others => '0');
begin
    clk <= not clk after 5 ns;

    counter : process (clk)
    begin
        if rising_edge(clk) then
            count <= count + 1;
        end if;
    end process;
end architecture sim;
