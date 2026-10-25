// probe: detection/sensor
// script: Far
// prim: X
// order: 2
// height: 100
// drive: none
//
// A sensor target of the Scanner's (see there), 100 m straight above prim
// A: beyond the documented 96 m sensor range. It names its prim
// "Target X", so a sweep can look for it alone.
default
{
    state_entry()
    {
        llSetObjectName("Target X");
    }
}
