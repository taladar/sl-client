// corpus: a binary operator's operands run right to left, a call's arguments,
// a list's elements and a vector's components left to right
// flavours: secondlife yengine
// Oracle: the book's simulator/lsl-engine.md, confirmed on aditi 2026-09-27:
// f(1) - f(2) + f(3) evaluates 3, 2, 1; a list, a vector and a call's
// arguments left to right. YEngine builds a list literal right to left
// (vm/eval-order, 2026-11-09). XEngine evaluates everything left to right,
// which the runtime does not model (its one OpenSim dialect is YEngine's).
integer f(integer n)
{
    print("f(" + (string)n + ")");
    return n;
}

integer three(integer a, integer b, integer c)
{
    return a + b + c;
}

default
{
    state_entry()
    {
        print("operators");
        integer sum = f(1) - f(2) + f(3);
        print("arguments");
        sum = three(f(4), f(5), f(6));
        print("list");
        list items = [f(7), f(8), f(9)];
        print("vector");
        vector v = <f(10), f(11), f(12)>;
    }
}
