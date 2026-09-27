// An assignment's left side is a variable, not an expression, so after a
// name nothing but the assignment can take the `=`: `c + 5 + e *= 4` is
// `c + 5 + (e *= 4)`, `0 < e = a - 3` is `0 < (e = a - 3)` and `!e = 0` is
// `!(e = 0)` — the shapes tailslide's `tltp/exporter.lsl` relies on.
default
{
    state_entry()
    {
        integer c = 1;
        integer e = 2;
        integer a = c + 5 + e *= 4;
        if (0 < e = a - 3)
        {
            llOwnerSay((string)(!e = 0));
        }
        vector v = <1.0, 2.0, 3.0>;
        llOwnerSay((string)(2.0 * v.z = 5.0));
    }
}
