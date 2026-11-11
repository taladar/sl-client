// probe: memory/stack
// script: Quiet
// order: 1
// drive: none
// skip-on: opensim-xengine, opensim-yengine
//
// Recurses without bound when the controller sends link message 1, with
// one integer argument and no locals, as memory/recursion's Narrow does,
// but makes next to nothing on the way: it writes its depth into the
// prim's description only every sixteenth level, and reads the memory
// functions only before it starts. So what it reaches is what the stack
// alone allows, with no garbage beside it.
//
// Left out on both OpenSim engines, where an unbounded recursion takes the
// simulator down (memory/recursion's narrow.lsl says how).
string ID = "memory/stack";
string NAME = "quiet";

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
        if (num == 1)
        {
            read("before", "");
            emit("result", " script=" + NAME + " value=" + (string)deep(0));
        }
    }
}
