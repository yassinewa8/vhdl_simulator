-- Assigns an integer to a std_logic_vector.
library ieee;
use ieee.std_logic_1164.all;

entity type_mismatch is
end entity;

architecture rtl of type_mismatch is
    signal v : std_logic_vector(7 downto 0);
begin
    v <= 5;
end architecture;
