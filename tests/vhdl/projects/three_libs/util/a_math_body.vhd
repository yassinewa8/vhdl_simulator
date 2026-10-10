-- Package body in its own file, named so it sorts before the package.
package body math_pkg is
    function double(x : integer) return integer is
    begin
        return 2 * x;
    end function;
end package body math_pkg;
