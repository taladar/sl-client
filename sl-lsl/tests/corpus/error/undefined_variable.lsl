// References a name that resolves to no local, global or library constant.
default
{
    state_entry()
    {
        llSay(0, undefinedVariable);
    }
}
