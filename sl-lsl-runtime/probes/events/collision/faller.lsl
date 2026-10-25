// probe: events/collision
// script: Faller
// prim: B
// order: 1
// height: 2
// drive: none
//
// The collision probe's falling prim, two metres above prim A. On `where`
// it answers `here`, so prim A learns its key. On `drop` it turns physical
// and falls; on `stop` it turns non-physical, goes back where it started,
// upright, and answers `report` with what it raised itself: its
// `collision_start` and `collision_end` events, its `land_collision_start`
// and `land_collision_end`, its `collision` events, and whether one of its
// collisions was with prim A. Prints nothing itself.
integer CUE = -730301;
integer BACK = -730302;

vector home;
key probe;
integer starts;
integer ends;
integer land_starts;
integer land_ends;
integer collisions;
integer hit;

default
{
    state_entry()
    {
        home = llGetPos();
        llSetStatus(STATUS_PHYSICS, FALSE);
        llListen(CUE, "", NULL_KEY, "");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (message == "where")
        {
            probe = id;
            llSay(BACK, "here");
        }
        else if (message == "drop")
        {
            starts = 0;
            ends = 0;
            land_starts = 0;
            land_ends = 0;
            collisions = 0;
            hit = 0;
            llSetStatus(STATUS_PHYSICS, TRUE);
        }
        else if (message == "stop")
        {
            llSetStatus(STATUS_PHYSICS, FALSE);
            llSetRegionPos(home);
            llSetRot(ZERO_ROTATION);
            llSay(BACK, "report " + (string)starts + " " + (string)ends
                + " " + (string)land_starts + " " + (string)land_ends
                + " " + (string)collisions + " " + (string)hit);
        }
    }

    collision_start(integer count)
    {
        ++starts;
        integer i;
        for (i = 0; i < count; ++i)
        {
            if (llDetectedKey(i) == probe)
            {
                hit = 1;
            }
        }
    }

    collision(integer count)
    {
        ++collisions;
    }

    collision_end(integer count)
    {
        ++ends;
    }

    land_collision_start(vector pos)
    {
        ++land_starts;
    }

    land_collision_end(vector pos)
    {
        ++land_ends;
    }
}
