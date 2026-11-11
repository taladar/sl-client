// probe: memory/stack
// script: Quiet limited
// order: 2
// drive: none
// skip-on: opensim-xengine, opensim-yengine
//
// Quiet under a limit lowered to 4096 bytes over what the script uses,
// when the controller sends link message 2.
//
// Left out on both OpenSim engines, where an unbounded recursion takes the
// simulator down (memory/recursion's narrow.lsl says how).
string ID = "memory/stack";
string NAME = "quiet-limited";

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

read(string rec, string fields)
{
    integer free = llGetFreeMemory();
    integer used = llGetUsedMemory();
    integer limit = llGetMemoryLimit();
    emit(rec, " script=" + NAME + fields + " used=" + (string)used
        + " free=" + (string)free + " limit=" + (string)limit);
}

integer deep(integer d)
{
    if ((d & 15) == 0) llSetObjectDesc((string)d);
    return deep(d + 1) + 1;
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2)
        {
            read("before", "");
            integer used = llGetUsedMemory();
            integer result = llSetMemoryLimit(used + 4096);
            read("set-limit", " asked=used+4096 result=" + (string)result);
            emit("result", " script=" + NAME + " value=" + (string)deep(0));
        }
    }
}
