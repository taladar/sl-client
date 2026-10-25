// probe: detection/sensor
// script: TargetU
// prim: U
// order: 3
// distance: 4
// drive: none
//
// A sensor target of the Scanner's (see there), 4 m east of prim A. It is
// scripted, which Second Life's sensor needs to see it, and names its prim
// "Target U", so a range or arc sweep can tell it from the rest.
default
{
    state_entry()
    {
        llSetObjectName("Target U");
    }
}
