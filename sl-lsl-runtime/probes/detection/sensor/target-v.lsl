// probe: detection/sensor
// script: TargetV
// prim: V
// order: 4
// distance: 3
// drive: none
//
// A sensor target of the Scanner's (see there), 3 m east of prim A. It is
// scripted, which Second Life's sensor needs to see it, and names its prim
// "Target V", so a range or arc sweep can tell it from the rest.
default
{
    state_entry()
    {
        llSetObjectName("Target V");
    }
}
