// corpus: what is false in a condition
// Oracle: the book's simulator/lsl-engine.md: 0, 0.0, ZERO_VECTOR,
// ZERO_ROTATION, "", the empty list and an invalid key are false, and the
// zero vector and rotation are usable in a condition at all. OpenSim makes
// a rotation of four zeros the identity (its LSL_Rotation constructor), so
// <0, 0, 0, 0> is false there (errors/math's Math 5 on both engines).
show(string name, integer truth)
{
    if (truth) print(name + " true"); else print(name + " false");
}

default
{
    state_entry()
    {
        if (0) show("0", 1); else show("0", 0);
        if (-1) show("-1", 1); else show("-1", 0);
        if (0.0) show("0.0", 1); else show("0.0", 0);
        if (0.001) show("0.001", 1); else show("0.001", 0);
        if ("") show("\"\"", 1); else show("\"\"", 0);
        if (" ") show("\" \"", 1); else show("\" \"", 0);
        if (ZERO_VECTOR) show("ZERO_VECTOR", 1); else show("ZERO_VECTOR", 0);
        if (<0, 0, 1>) show("<0, 0, 1>", 1); else show("<0, 0, 1>", 0);
        if (ZERO_ROTATION) show("ZERO_ROTATION", 1); else show("ZERO_ROTATION", 0);
        if (<0, 0, 0, 0>) show("<0, 0, 0, 0>", 1); else show("<0, 0, 0, 0>", 0);
        if ([]) show("[]", 1); else show("[]", 0);
        if ([0]) show("[0]", 1); else show("[0]", 0);
    }
}
