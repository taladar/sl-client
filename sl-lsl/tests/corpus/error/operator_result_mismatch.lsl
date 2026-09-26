// `vector * vector` is the dot product — a float — so it cannot fill the
// vector parameter of llSetPos.
default
{
    state_entry()
    {
        vector v = <1.0, 2.0, 3.0>;
        llSetPos(v * v);
    }
}
