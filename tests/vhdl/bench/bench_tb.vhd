-- Generates a big waveform for the wave crate's benchmark
-- (crates/wave/examples/bench.rs). The run length sets the size: every
-- 10 ns gives 2 clock changes, 1 counter change, 1 change of a 128-bit
-- bus and a quarter of an enum change. 50 ms is about 21 million changes,
-- 10 million of them on the clock.
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity bench_tb is
end entity bench_tb;

architecture sim of bench_tb is
    type state_t is (IDLE, LOAD, SHIFT, DONE);

    signal clk   : std_logic := '0';
    signal count : unsigned(31 downto 0) := (others => '0');
    -- A 128-bit linear feedback shift register: changes every cycle, in a
    -- pattern that doesn't compress as well as a counter.
    signal wide  : std_logic_vector(127 downto 0) := (0 => '1', others => '0');
    signal state : state_t := IDLE;
    signal slow  : std_logic := '0';
begin
    clk <= not clk after 5 ns;
    slow <= not slow after 1 us;

    logic : process (clk)
    begin
        if rising_edge(clk) then
            count <= count + 1;
            wide <= wide(126 downto 0) & (wide(127) xor wide(125) xor wide(100) xor wide(98));
            if count(1 downto 0) = "11" then
                case state is
                    when IDLE  => state <= LOAD;
                    when LOAD  => state <= SHIFT;
                    when SHIFT => state <= DONE;
                    when DONE  => state <= IDLE;
                end case;
            end if;
        end if;
    end process;
end architecture sim;
