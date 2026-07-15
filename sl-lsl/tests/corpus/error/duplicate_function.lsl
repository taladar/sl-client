// Two global functions share a name; LSL has no overloading.
integer dup()
{
    return 1;
}

integer dup()
{
    return 2;
}

default
{
    state_entry()
    {
        llSay(0, (string)dup());
    }
}
