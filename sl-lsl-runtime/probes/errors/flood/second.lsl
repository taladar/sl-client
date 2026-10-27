// probe: errors/flood
// script: Second
// order: 2
// drive: none
//
// A second script in prim A. Asked by "Flooder" in the middle of its
// flood, it makes one error of its own: whether a flood is the script's
// or the object's.
string ID = "errors/flood";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1)
        {
            emit("asked", " script=second");
            llGetScriptState("Second asks");
        }
    }
}
