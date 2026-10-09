-- Never stops by itself. Used to test cancelling a running simulation.
entity endless_tb is
end entity;

architecture sim of endless_tb is
    signal clk : bit := '0';
begin
    clk <= not clk after 5 ns;
end architecture;
