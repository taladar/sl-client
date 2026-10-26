// probe: memory/recursion
// script: Limited
// order: 3
// drive: none
// skip-on: opensim-xengine, opensim-yengine
//
// Recurses without bound when the controller sends link message 3,
// writing each level's depth into the prim's description before it goes
// deeper, so the controller reads the deepest level reached after the
// script has stopped. Reads the memory functions at depth 0 and at each
// power of two.
// Narrow's recursion, after llSetMemoryLimit asked for 4096 bytes more
// than the script uses: whether the stack counts against the limit.
//
// Left out on both OpenSim engines, where it takes the simulator down. On
// XEngine any unbounded recursion overflows the simulator's own native
// stack and the whole OpenSim process aborts (errors/runtime-errors,
// 2026-10-23). YEngine catches a recursion that calls nothing
// (errors/runtime-errors), but this one calls llSetObjectDesc at every
// level, and the native stack then overflows inside YEngine's own stack
// check (XMRInstAbstract.CheckRunStack) and the process aborts just the
// same (2026-10-26).
string ID = "memory/recursion";
string NAME = "limited";

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
    llSetObjectDesc((string)d);
    if ((d & (d - 1)) == 0) read("depth", " depth=" + (string)d);
    return deep(d + 1) + 1;
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 3)
        {
            read("before", "");
            integer used = llGetUsedMemory();
            integer result = llSetMemoryLimit(used + 4096);
            read("set-limit", " asked=used+4096 result=" + (string)result);
            emit("result", " script=" + NAME + " value=" + (string)deep(0));
        }
    }
}
