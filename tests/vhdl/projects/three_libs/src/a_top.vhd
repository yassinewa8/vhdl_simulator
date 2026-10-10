-- Uses a context from this library and instantiates a component, both
-- defined in files that sort after this one.
context work.design_ctx;

entity top is
    generic (OFFSET : integer := 0);
    port (x : in integer; y : out integer);
end entity top;

architecture rtl of top is
    component adder is
        port (a, b : in integer; sum : out integer);
    end component;
    signal doubled : integer;
begin
    doubled <= double(x);
    u_adder : adder port map (a => doubled, b => OFFSET, sum => y);
end architecture rtl;
