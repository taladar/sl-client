// probe: detection/sensor
// script: TargetW
// prim: W
// order: 5
// height: 3.5
// drive: none
//
// A sensor target of the Scanner's (see there), 3.5 m straight above
// prim A, outside a narrow arc about its forward axis. It is scripted,
// which Second Life's sensor needs to see it, and names its prim
// "Target W", so a range or arc sweep can tell it from the rest.
default
{
    state_entry()
    {
        llSetObjectName("Target W");
    }
}
