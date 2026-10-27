// probe: inventory/rez
// script: Child
// object: Child
// order: 1
// drive: none
//
// What the object in prim A's inventory carries. Rezzed, it says on a
// channel what it was given and where it stands — the start parameter as
// on_rez and llGetStartParameter have it, its key, the rezzer its details
// name, the time, its position, rotation and velocity, whether it dies at
// the region's edge — and dies. Its state_entry says so too, which a grid
// that starts the script afresh on a rez runs (where it was saved, nothing
// listens yet).
integer CHANNEL = -7341;

default
{
    state_entry()
    {
        llSay(CHANNEL, "entry|" + (string)llGetStartParameter());
    }

    on_rez(integer param)
    {
        key rezzer = llList2Key(llGetObjectDetails(llGetKey(), [OBJECT_REZZER_KEY]), 0);
        llSay(CHANNEL, llDumpList2String(["rez", param, llGetStartParameter(), llGetKey(),
            rezzer, llGetTimestamp(), llGetPos(), llGetRot(), llGetVel(),
            llGetStatus(STATUS_DIE_AT_EDGE)], "|"));
        llDie();
    }
}
