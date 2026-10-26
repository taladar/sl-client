// probe: memory/collision
// script: Full list
// order: 3
// drive: none
//
// Appends integers ten at a time to a global list when the controller
// sends link message 3, writing the count it is making into the prim's
// description before each append and reading the memory functions every
// 500. Stops at 20 000, past Second Life's 64 KB. Ten at a time because
// Second Life gives a busy script a slice of each frame, and one append
// at a time would take minutes.
string ID = "memory/collision";
string NAME = "full-list";

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

list held;

grow(integer bound)
{
    held = [];
    integer n;
    for (n = 10; n <= bound; n += 10)
    {
        llSetObjectDesc((string)n);
        held += [n, n, n, n, n, n, n, n, n, n];
        if (n % 500 == 0) read("grown", " count=" + (string)n);
    }
    llSetObjectDesc((string)llGetListLength(held));
    emit("no-collision", " script=" + NAME + " count="
        + (string)llGetListLength(held));
    llMessageLinked(LINK_THIS, 100, "", "");
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 3)
        {
            read("before", "");
            grow(20000);
        }
    }
}
