// corpus: && and || evaluate both operands, the right one first
// Oracle: the book's simulator/lsl-engine.md, confirmed on aditi 2026-09-27:
// f(0) && f(11) evaluates both sides, 11 first.
integer f(integer n)
{
    print("f(" + (string)n + ")");
    return n;
}

default
{
    state_entry()
    {
        if (f(0) && f(11)) print("and: true"); else print("and: false");
        if (f(1) || f(12)) print("or: true"); else print("or: false");
    }
}
