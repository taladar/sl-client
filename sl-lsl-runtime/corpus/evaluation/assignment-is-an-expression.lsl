// corpus: an assignment yields its value, and binds tighter than it reads
// Oracle: the book's simulator/lsl-engine.md: c + 5 + e *= 4 parses as
// c + 5 + (e *= 4), and integer *= float stores the integer.
integer c = 1;
integer e = 2;
integer stored = 3;

default
{
    state_entry()
    {
        print((string)(c + 5 + e *= 4));
        print((string)e);
        stored *= 1.5;
        print((string)stored);
        integer x;
        integer y;
        x = y = 7;
        print((string)(x + y));
    }
}
