-- Instantiates an entity that doesn't exist (fails at analysis or elaboration).
entity missing_unit is
end entity;

architecture rtl of missing_unit is
begin
    u0 : entity work.does_not_exist;
end architecture;
