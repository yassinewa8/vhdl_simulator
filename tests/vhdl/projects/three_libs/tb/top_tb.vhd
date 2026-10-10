library design_lib;

entity top_tb is
    generic (OFFSET : integer := 0);
end entity top_tb;

architecture sim of top_tb is
    signal x, y : integer := 0;
begin
    dut : entity design_lib.top
        generic map (OFFSET => OFFSET)
        port map (x => x, y => y);

    check : process
    begin
        x <= 10;
        wait for 10 ns;
        report "y = " & integer'image(y);
        assert y = 2 * 10 + OFFSET report "wrong sum" severity failure;
        wait;
    end process;
end architecture sim;
