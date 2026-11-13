// Does not compile, on purpose: the statement lacks its semicolon and names a
// variable nobody declared, so a script editor has errors to show.
default
{
    state_entry()
    {
        llSay(0, "never said " + undeclared)
    }
}
