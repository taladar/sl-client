// probe: memory/sizes
// script: Globals none
// order: 1
// drive: none
//
// No globals but its own two: the base the other Globals scripts are
// read against. Reads llGetUsedMemory twice when the
// probe's leader sends link message 1, prints both and answers with link
// message 2.
string ID = "memory/sizes";
string KIND = "none";

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num != 1) return;
        integer a = llGetUsedMemory();
        integer b = llGetUsedMemory();
        llOwnerSay("PROBE " + ID + " rec=globals kind=" + KIND + " count=32 used="
            + (string)a + "," + (string)b);
        llMessageLinked(LINK_THIS, 2, KIND, NULL_KEY);
    }
}
