// probe: memory/collision
// script: Doubling LSO
// order: 2
// drive: none
// target: lsl2
//
// Doubling compiled to LSO on Second Life, sent link message 2.
string ID = "memory/collision";
string NAME = "doubling-lso";

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

string text;

grow()
{
    text = "a";
    integer step;
    for (step = 1; step <= 22; ++step)
    {
        llSetObjectDesc((string)step);
        text += text;
        read("grown", " step=" + (string)step
            + " length=" + (string)llStringLength(text));
    }
    emit("no-collision", " script=" + NAME + " length="
        + (string)llStringLength(text));
    llMessageLinked(LINK_THIS, 100, "", "");
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2)
        {
            read("before", "");
            grow();
        }
    }
}
