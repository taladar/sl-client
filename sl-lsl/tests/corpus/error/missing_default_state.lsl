// A script with states but no `default` state — LSL requires one.
state waiting
{
    state_entry()
    {
        llSay(0, "no default here");
    }
}
