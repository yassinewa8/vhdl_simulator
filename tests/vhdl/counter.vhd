-- Simple counter and testbench. First test design; used from Phase 1 on.

library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity counter is
    generic (WIDTH : positive := 8);
    port (
        clk   : in  std_logic;
        rst   : in  std_logic;
        count : out unsigned(WIDTH - 1 downto 0)
    );
end entity;

architecture rtl of counter is
    signal count_r : unsigned(WIDTH - 1 downto 0) := (others => '0');
begin
    process (clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                count_r <= (others => '0');
            else
                count_r <= count_r + 1;
            end if;
        end if;
    end process;

    count <= count_r;
end architecture;

library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity counter_tb is
end entity;

architecture sim of counter_tb is
    signal clk   : std_logic := '0';
    signal rst   : std_logic := '1';
    signal count : unsigned(7 downto 0);
    signal done  : boolean := false;
begin
    uut : entity work.counter
        port map (clk => clk, rst => rst, count => count);

    clk <= not clk after 5 ns when not done;

    stimulus : process
    begin
        wait for 20 ns;
        rst <= '0';
        wait for 100 ns;
        report "count = " & integer'image(to_integer(count));
        done <= true;
        wait;
    end process;
end architecture;
