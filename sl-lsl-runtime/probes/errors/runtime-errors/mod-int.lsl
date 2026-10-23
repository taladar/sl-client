// probe: errors/runtime-errors
// script: Mod int
// prim: A
// order: 2
// drive: none
//
// Takes an integer modulo zero.
string ID = "errors/runtime-errors";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 3) emit("fault", " script=mod-int result=" + (string)(7 % (integer)msg));
    }
}
