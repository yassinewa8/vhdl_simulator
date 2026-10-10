library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

library osvvm;
context osvvm.OsvvmContext;

-- Checks the parity block against a reference model with 100 random
-- words, using OSVVM's random numbers and alert/affirm reporting.
entity parity_tb is
end entity parity_tb;

architecture sim of parity_tb is
    signal data   : std_logic_vector(7 downto 0) := (others => '0');
    signal parity : std_logic;
begin
    dut : entity work.parity
        port map (data => data, parity => parity);

    check : process
        variable random   : RandomPType;
        variable expected : std_logic;
    begin
        SetTestName("parity_tb");
        random.InitSeed(random'instance_name);

        for i in 1 to 100 loop
            data <= random.RandSlv(8);
            wait for 10 ns;
            expected := '0';
            for bit_index in data'range loop
                expected := expected xor data(bit_index);
            end loop;
            AffirmIfEqual(parity, expected, "parity of " & to_hstring(data));
        end loop;

        -- ReportAlerts rather than EndOfTestReports: the latter expects
        -- files made by OSVVM's own Tcl scripts.
        ReportAlerts;
        std.env.stop;
        wait;
    end process;
end architecture sim;
