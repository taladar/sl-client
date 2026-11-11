// probe: memory/stack
// script: Litter
// order: 4
// drive: none
// skip-on: opensim-xengine, opensim-yengine
//
// Quiet, but each level makes a string of 100 characters and more and
// drops it, when the controller sends link message 4: whether garbage a
// script has not had collected cuts its depth.
//
// Left out on both OpenSim engines, where an unbounded recursion takes the
// simulator down (memory/recursion's narrow.lsl says how).
string ID = "memory/stack";
string NAME = "litter";
string PAD;

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
    llStringLength(PAD + (string)d);
    return deep(d + 1) + 1;
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 4)
        {
            integer i;
            for (i = 0; i < 10; ++i) PAD += "xxxxxxxxxx";
            read("before", "");
            emit("result", " script=" + NAME + " value=" + (string)deep(0));
        }
    }
}
