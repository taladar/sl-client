// Physical: falls on to whatever is below it and says what it hit. Touched, it
// is lifted three metres and dropped again.
default
{
    state_entry()
    {
        llSetText("Touch to drop me again", <1.0, 1.0, 1.0>, 1.0);
        llSetStatus(STATUS_PHYSICS, TRUE);
    }

    collision_start(integer count)
    {
        llSay(0, "Bumped " + llDetectedName(0));
    }

    land_collision_start(vector where)
    {
        llSay(0, "Hit the ground");
    }

    touch_start(integer count)
    {
        llSetStatus(STATUS_PHYSICS, FALSE);
        llSetPos(llGetPos() + <0.0, 0.0, 3.0>);
        llSetStatus(STATUS_PHYSICS, TRUE);
    }
}
