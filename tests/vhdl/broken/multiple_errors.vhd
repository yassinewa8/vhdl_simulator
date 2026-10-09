-- Two independent errors in one file, to check we report both.
library ieee;
use ieee.std_logic_1164.all;

entity multiple_errors is
end entity;

architecture rtl of multiple_errors is
    signal a : std_logic;
    signal n : integer;
begin
    a <= missing_one;
    n <= missing_two;
end architecture;
