// probe: memory/collision
// script: Doubling
// order: 1
// drive: none
//
// Doubles a global string from one character when the controller sends
// link message 1, writing the step into the prim's description before
// each doubling and reading the memory functions after it. Stops at 2^22
// characters, which is past every grid's limit but XEngine's (it has
// none), so the host is not run out of memory there.
string ID = "memory/collision";
string NAME = "doubling";

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
        if (num == 1)
        {
            read("before", "");
            grow();
        }
    }
}
