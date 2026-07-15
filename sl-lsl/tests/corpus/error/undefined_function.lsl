// Calls a name that is neither a user function nor a library function.
default
{
    state_entry()
    {
        llNotARealFunction(0, "oops");
    }
}
