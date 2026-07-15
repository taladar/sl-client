// An event handler (void context) cannot return a value.
default
{
    state_entry()
    {
        return 5;
    }
}
