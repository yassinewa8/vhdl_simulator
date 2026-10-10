entity adder is
    port (a, b : in integer; sum : out integer);
end entity adder;

architecture rtl of adder is
begin
    sum <= a + b;
end architecture rtl;
