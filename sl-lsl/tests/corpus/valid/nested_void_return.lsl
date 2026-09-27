// Returning the result of a void call from a void function or an event
// handler is legal when the `return` is nested in an `if` or a loop, and an
// error only directly in the body (tailslide's `void_return.lsl`).
nothing()
{
    if (TRUE)
        return llOwnerSay("legal");
}

default
{
    state_entry()
    {
        nothing();
        while (TRUE)
            return llOwnerSay("legal");
    }
}
