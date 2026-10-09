-- Uses a signal that was never declared.
library ieee;
use ieee.std_logic_1164.all;

entity undeclared is
end entity;

architecture rtl of undeclared is
    signal a : std_logic;
begin
    a <= b;
end architecture;
