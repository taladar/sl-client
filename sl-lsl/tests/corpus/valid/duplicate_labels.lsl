// Two labels of one name in one body compile on the grid (a `jump` then lands
// on whichever its compiler picks), so the pass may warn about it but must not
// call it an error.
default
{
    state_entry()
    {
        if (llFrand(1.0) < 0.5)
        {
            jump done;
            @done;
        }
        @done;
        llOwnerSay("after");
    }
}
