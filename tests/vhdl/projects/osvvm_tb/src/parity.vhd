library ieee;
use ieee.std_logic_1164.all;

-- Even parity of an 8-bit word: '1' when the number of ones is odd.
entity parity is
    port (
        data   : in  std_logic_vector(7 downto 0);
        parity : out std_logic
    );
end entity parity;

architecture rtl of parity is
begin
    parity <= xor data;
end architecture rtl;
