// probe: inventory/newer
// script: Child
// object: Child
// order: 1
// drive: none
//
// What the object in prim A's inventory carries: rezzed, it says on a channel
// its start parameter and start string, and waits to be derezzed (it is
// rezzed temporary, so it goes all the same).
integer CHANNEL = -7342;

default
{
    on_rez(integer param)
    {
        llSay(CHANNEL, llDumpList2String(["rez", param, llGetStartString(), llGetKey()], "|"));
    }
}
