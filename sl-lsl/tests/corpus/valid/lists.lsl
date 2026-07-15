// List construction and library list operations — a heterogeneous list literal
// and a couple of list builtins, none of which should draw a diagnostic.
default
{
    state_entry()
    {
        list data = [1, "two", 3.0, llGetOwner()];
        integer count = llGetListLength(data);
        string joined = llList2CSV(data);
        llSay(0, joined + " has " + (string)count + " entries");
    }
}
