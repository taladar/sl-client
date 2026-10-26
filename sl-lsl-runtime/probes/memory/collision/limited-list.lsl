// probe: memory/collision
// script: Limited list
// order: 4
// drive: none
//
// Full list's growth after llSetMemoryLimit asked for 8192 bytes more than
// the script uses, when the controller sends link message 4. Stops at
// 4 000.
string ID = "memory/collision";
string NAME = "limited-list";

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
        if (num == 4)
        {
            read("before", "");
            integer used = llGetUsedMemory();
            integer result = llSetMemoryLimit(used + 8192);
            read("set-limit", " asked=used+8192 result=" + (string)result);
            grow(4000);
        }
    }
}
