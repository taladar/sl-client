// probe: errors/runtime-errors
// script: Div float
// prim: A
// order: 3
// drive: none
//
// Divides a float by zero.
string ID = "errors/runtime-errors";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 4) emit("fault", " script=div-float result=" + (string)(1.0 / (float)msg));
    }
}
