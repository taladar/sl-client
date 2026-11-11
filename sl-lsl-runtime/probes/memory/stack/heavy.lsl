// probe: memory/stack
// script: Heavy
// order: 3
// drive: none
// skip-on: opensim-xengine, opensim-yengine
//
// Quiet over a heap that holds 2 000 integers in a global list (some
// 32 000 bytes) first, when the controller sends link message 3: whether
// what the heap holds cuts the depth the default limit allows. The list is
// made ten integers at a time, each boxed on its own, so it is made well
// inside the controller's ten seconds on every grid.
//
// Left out on both OpenSim engines, where an unbounded recursion takes the
// simulator down (memory/recursion's narrow.lsl says how).
string ID = "memory/stack";
string NAME = "heavy";
list held;

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
        if (num == 3)
        {
            integer i;
            for (i = 0; i < 2000; i += 10)
                held += [i, i, i, i, i, i, i, i, i, i];
            read("before", " count=" + (string)llGetListLength(held));
            emit("result", " script=" + NAME + " value=" + (string)deep(0));
        }
    }
}
