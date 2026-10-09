-- Prints a report at each severity, then fails an assertion.
-- Used to capture NVC's simulation-time message format.
entity messages_tb is
end entity;

architecture sim of messages_tb is
begin
    process
    begin
        report "a note message";
        wait for 10 ns;
        report "a warning message" severity warning;
        wait for 5 ns;
        report "an error message" severity error;
        wait for 1 ps;
        assert 1 = 2 report "assertion with message" severity error;
        assert false;
        wait for 3 us;
        report "a failure message" severity failure;
        report "not reached";
        wait;
    end process;
end architecture;
