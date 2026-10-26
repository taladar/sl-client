// probe: memory/object
// script: Limited
// order: 3
// drive: none
//
// A Mono script that lowers its memory limit to 16 384 bytes as it starts,
// and raises it to 32 768 when the reader sends link message 2.
string ID = "memory/object";

set_limit(integer asked)
{
    integer result = llSetMemoryLimit(asked);
    integer now = llGetMemoryLimit();
    llOwnerSay("PROBE " + ID + " rec=limit script=limited asked="
        + (string)asked + " result=" + (string)result
        + " limit=" + (string)now);
}

default
{
    state_entry()
    {
        set_limit(16384);
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2) set_limit(32768);
    }
}
