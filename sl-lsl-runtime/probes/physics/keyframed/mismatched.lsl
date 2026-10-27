// probe: physics/keyframed
// script: Mismatched
// prim: C
// order: 2
// distance: 6
// drive: none
//
// The Probe's neighbour in prim C, six metres east of prim A (see there):
// it makes its prim's physics shape convex and calls llSetKeyframedMotion
// once, as soon as it is saved, with a frame of translation, rotation and
// time under KFM_DATA KFM_TRANSLATION, which takes two elements a frame, and
// prints how far the prim went a second and a half later. It is a script of its own because OpenSim throws on it,
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
        emit("try", " label=translation-only-three");
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0], [KFM_DATA, KFM_TRANSLATION]);
        llSetTimerEvent(1.5);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        vector p = llGetPos();
        emit("outcome", " label=translation-only-three moved=" + (string)(llVecDist(p, home) > 0.01));
        llSetKeyframedMotion([], [KFM_COMMAND, KFM_CMD_STOP]);
        llSetRegionPos(home);
    }
}
