// probe: physics/keyframed
// script: Swapped
// prim: B
// order: 1
// distance: 3
// drive: none
//
// The Probe's neighbour in prim B, three metres east of prim A (see there):
// it makes its prim's physics shape convex and calls llSetKeyframedMotion
// once, as soon as it is saved, with a frame whose rotation and translation
// are in each other's places, and prints how far the prim went a second and
// a half later. It is a script of its own because OpenSim throws on it,
// which stops the script that asked.
string ID = "physics/keyframed";

vector home;

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

default
{
    state_entry()
    {
        home = llGetPos();
        llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, PRIM_PHYSICS_SHAPE_CONVEX]);
        emit("try", " label=types-swapped");
        llSetKeyframedMotion([ZERO_ROTATION, <0.0, 0.0, 0.5>, 1.0], []);
        llSetTimerEvent(1.5);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        vector p = llGetPos();
        emit("outcome", " label=types-swapped moved=" + (string)(llVecDist(p, home) > 0.01));
        llSetKeyframedMotion([], [KFM_COMMAND, KFM_CMD_STOP]);
        llSetRegionPos(home);
    }
}
