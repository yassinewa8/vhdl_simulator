-- Missing semicolon after the signal declaration.
library ieee;
use ieee.std_logic_1164.all;

entity syntax_error is
end entity;

architecture rtl of syntax_error is
    signal a : std_logic
begin
end architecture;
