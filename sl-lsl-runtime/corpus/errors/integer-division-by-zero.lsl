// corpus: integer division by zero is a run-time Math Error that stops the
// script on Second Life
// Oracle: the value model ("integer division by zero is a run-time error").
integer reached;

default
{
    state_entry()
    {
        integer zero = 0;
        print("before");
        print((string)(1 / zero));
        reached = TRUE;
        print("after");
    }
}
