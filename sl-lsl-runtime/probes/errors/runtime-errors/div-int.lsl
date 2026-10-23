// probe: errors/runtime-errors
// script: Div int
// prim: A
// order: 1
// drive: none
//
// Divides an integer by zero in a second state, after setting a global, so
// the restart shows which state and globals a faulted script keeps.
string ID = "errors/runtime-errors";

integer g = 1;

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
            g = 42;
            state armed;
        }
        else if (num == 100) emit("answer", " script=div-int state=default g=" + (string)g);
    }
}

state armed
{
    state_entry()
    {
        llMessageLinked(LINK_THIS, 2, "0", "");
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2)
        {
            emit("fault", " script=div-int step=before");
            emit("fault", " script=div-int result=" + (string)(1 / (integer)msg));
            emit("fault", " script=div-int step=after");
        }
        else if (num == 100) emit("answer", " script=div-int state=armed g=" + (string)g);
    }
}
