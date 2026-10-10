-- One signal of each kind the waveform viewer has to show, changing at
-- known times (every 10 ns), plus a signal inside a nested block.
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity types_tb is
end entity types_tb;

architecture sim of types_tb is
    type state_t is (IDLE, START, DATA, STOP_BIT);
    type pair_t is record
        valid : std_logic;
        count : unsigned(3 downto 0);
    end record;

    signal sl    : std_logic;                         -- starts 'U'
    signal bus8  : std_logic_vector(7 downto 0);      -- starts all 'U'
    signal snum  : signed(7 downto 0) := (others => '0');
    signal state : state_t := IDLE;
    signal count : integer range 0 to 1000 := 0;
    signal flag  : boolean := false;
    signal b     : bit := '0';
    signal rec   : pair_t := ('0', "0000");
    signal r     : real := 0.0;
    signal t     : time := 0 ns;
begin
    stimulus : process
    begin
        wait for 10 ns;
        sl <= 'X';
        bus8 <= "01XZWLH-";
        wait for 10 ns;
        sl <= 'Z';
        bus8 <= x"A5";
        snum <= to_signed(-3, 8);
        state <= START;
        wait for 10 ns;
        sl <= 'W';
        state <= DATA;
        count <= 42;
        flag <= true;
        b <= '1';
        wait for 10 ns;
        sl <= '-';
        state <= STOP_BIT;
        rec <= ('1', "1010");
        wait for 10 ns;
        sl <= 'L';
        r <= 3.25;
        t <= 7 ns;
        count <= 1000;
        wait for 10 ns;
        sl <= 'H';
        wait;
    end process;

    nested : block
        signal inner : std_logic := '0';
    begin
        inner <= not inner after 7 ns when now < 40 ns;
    end block;
end architecture sim;
