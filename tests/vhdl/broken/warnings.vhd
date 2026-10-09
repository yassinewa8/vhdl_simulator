-- Analyses with warnings but no errors when given --relaxed --check-synthesis:
-- a shared variable without a protected type, and an incomplete sensitivity list.
library ieee;
use ieee.std_logic_1164.all;

entity warnings is
    port (a, b : in std_logic; y : out std_logic);
end entity;

architecture rtl of warnings is
    shared variable sv : integer;
begin
    process (a)
    begin
        y <= a and b;
    end process;
end architecture;
